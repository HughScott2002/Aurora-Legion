# ITE 8295 hardware profiles, Fn+Space, and state readback

Date: 2026-07-25

Scope: Lenovo Legion 4-zone RGB keyboards driven by the ITE 8295 USB
controller (VID 048d, PIDs c955/c963/c965/c973/c975/c983/c984/c985/
c993/c994/c995), the protocol `0xCC 0x16 [effect] [speed] [brightness]
[12 RGB bytes] ...` used by Aurora and every open-source tool. All
claims cite the owning source. Statements marked "inference" or
"community report" are not confirmed by code or vendor documentation.

## TL;DR

1. **Fn+Space cycles an EC-owned lighting profile counter, not just
   brightness.** Lenovo's own manual for these keyboards says Fn+Space
   is used "to cycle through factory pre-configured lighting effects
   (profiles)"
   ([Lenovo Legion Slim 7 feature guide](https://download.lenovo.com/pccbbs/pubs/legion_slim_7/html/html_en/explore_lightingEffects.html)).
   The only project that reads the counter observed values where **4 =
   off**, implying three "on" profiles plus off, which matches the
   "about 3" observation
   ([maniac103/lenovo-kbd-backlight `kbd_bl_api.py`](https://github.com/maniac103/lenovo-kbd-backlight/blob/master/service/kbd_bl_api.py)).
   On Windows the visible cycling is reimplemented in software:
   Vantage or LenovoLegionToolkit listens for a WMI event and applies
   its *own* stored presets. On white-backlight (non-RGB) models
   Fn+Space is a plain off/medium/bright brightness cycle handled by
   the EC. No evidence of different RGB-model behavior across the
   2020-2024 PIDs; they all share one protocol.

2. **Yes, partial state readback exists and is publicly used by
   exactly one project.** `GET_FEATURE` on report ID `0xCC` succeeds;
   the byte after the report ID is the current profile index (4 = off).
   maniac103's daemon calls
   `self.dev.get_feature_report(0xcc, 32)[1]` and polls it after each
   Fn+Space to learn the new state. Nobody has published what the
   remaining 30 bytes contain; whether effect/speed/colors are readable
   is unknown. No input report on Fn+Space has ever been reported for
   this interface; maniac103 polls instead. 4JX, OpenRGB and
   LenovoLegionToolkit never read from the device at all.

3. **Yes, Fn+Space is observable on Linux, as an ACPI/WMI event, not a
   key event.** Verified end to end in the DSDT of a Legion 5 15ACH6H:
   EC query `_QDE` does `Notify (GZFD, 0xE6)`, and GZFD's `_WDG` maps
   notify 0xE6 to WMI event GUID
   `D320289E-8FEA-41E0-86F9-811D83151B5F`, named
   `LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT` in Lenovo's MOF. No
   mainline or LenovoLegionLinux driver binds that GUID, and the kernel
   WMI core forwards every WMI event to the ACPI netlink socket, so a
   userspace daemon can listen for it (class "wmi", bus_id
   "PNP0C14:01"). It never appears as an evdev key, and the second ITE
   device 048d:c103 "ITE Device(8910)" has no published role in it.

4. **Every project either re-applies state or ignores the problem.**
   LenovoLegionToolkit (Windows) is the reference: it subscribes to the
   WMI event, claims ownership with the GameZone WMI method
   `SetLightControlOwner`, and on each Fn+Space advances its own preset
   list and re-sends the full `0xCC 0x16` feature report; it also
   re-applies on resume from sleep and returns ownership to firmware on
   exit. maniac103 (Linux) listens on ACPI netlink, polls the profile
   byte, and re-applies its single configured state (collapsing the
   hotkey to on/off). 4JX/L5P-Keyboard-RGB does nothing about Fn+Space:
   its author states Fn keys "are not registered by the OS", and the
   offered workaround is the app's own global shortcut plus manual
   autostart. OpenRGB is fire-and-forget with no sync handling.

**Onboard storage:** the controller/EC keeps lighting state without any
software. A user whose lighting glitched reported the stuck effect
persisting "even in BIOS" until an EC reset via the NOVO button
([4JX issue #52](https://github.com/4JX/L5P-Keyboard-RGB/issues/52)),
and Lenovo calls the cycled profiles "factory pre-configured". No
public reverse engineering has found a SELECT/SWITCH-profile HID
command; `0x16` is the only documented second byte on this device.

## 1. What Fn+Space cycles

Confirmed facts:

- Lenovo's feature guide for multi-color Legion keyboards:
  "Use keyboard shortcut Fn + Space to cycle through factory
  pre-configured lighting effects (profiles)."
  ([download.lenovo.com](https://download.lenovo.com/pccbbs/pubs/legion_slim_7/html/html_en/explore_lightingEffects.html)).
  It does not state the profile count.
- The EC exposes the current profile as a readable counter and the
  observed off value is 4
  ([kbd_bl_api.py lines 17-18 and 40](https://github.com/maniac103/lenovo-kbd-backlight/blob/master/service/kbd_bl_api.py)):
  `get_current_profile()` returns
  `self.dev.get_feature_report(0xcc, 32)[1]` and the daemon treats
  `profile == 4` as "backlight off". Three on-profiles plus off is the
  reading most consistent with that code.
- maniac103's README describes the Windows behavior as "moving between
  4 profiles" and notes that with his Linux daemon the same key
  becomes an on/off toggle by design
  ([README](https://github.com/maniac103/lenovo-kbd-backlight/blob/master/README.md)).
- 4JX users describe the same thing: "we use the Fn+space to cycle
  through the 4 profiles"
  ([issue #167](https://github.com/4JX/L5P-Keyboard-RGB/issues/167)),
  and the project's bug template asks reporters to confirm "I can
  cycle through the keyboard profiles with FN+Space"
  ([bug_report.yml](https://github.com/4JX/L5P-Keyboard-RGB/blob/main/.github/ISSUE_TEMPLATE/bug_report.yml)).
- On Windows the *visible* cycling is software. LenovoLegionToolkit
  advances a preset enum defined entirely in its own settings store
  (`Off, One, Two, Three, Four`) each time the WMI event fires, then
  writes the corresponding `0xCC 0x16` report
  ([Enums.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/Enums.cs),
  [RGBKeyboardBacklightController.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/Controllers/RGBKeyboardBacklightController.cs)).
  The count of software profiles is therefore whatever the tool
  chooses; only the EC counter (1..4) is hardware.
- White-backlight Legion models are brightness-only: LenovoLegionLinux
  documents "off/on white backlight or off/medium/bright white
  backlight" handled via WMI method GUID
  `8C5B9127-ECD4-4657-980F-851019F99CA5` ("access the keyboard
  backlight with 3 states")
  ([legion-laptop.c](https://github.com/johnfanv2/LenovoLegionLinux/blob/main/kernel_module/legion-laptop.c),
  [README](https://github.com/johnfanv2/LenovoLegionLinux#keyboard-backlight)).
  A Legion 5 15ACH6 owner with the white keyboard confirms Fn+Space
  changes intensity there
  ([LLL issue #58](https://github.com/johnfanv2/LenovoLegionLinux/issues/58)).

Model years: the 2020-2024 4-zone PIDs (c955 through c995) share one
protocol and one driver entry in every tool
([4JX driver/src/lib.rs](https://github.com/4JX/L5P-Keyboard-RGB/blob/main/driver/src/lib.rs)).
No source distinguishes Fn+Space semantics between those years. Units
whose only ITE device is 048d:c101/c102/c103 "ITE Device(8910)" are
white-backlight or otherwise unsupported SKUs, not 4-zone RGB
([4JX issue #215](https://github.com/4JX/L5P-Keyboard-RGB/issues/215),
[issue #267](https://github.com/4JX/L5P-Keyboard-RGB/issues/267)).

Inference (not directly proven): the EC cycles its counter and applies
the stored profile itself when no software has claimed ownership. The
strongest evidence is that maniac103's Linux daemon works by *waiting
for the counter to change on its own* after the ACPI event, then
overwriting the result, and that lighting state survives into BIOS
where no OS software runs
([4JX issue #52](https://github.com/4JX/L5P-Keyboard-RGB/issues/52)).
A likely-related community report: a machine where Vantage was broken
still showed the on-screen Fn+Space indicator while the lighting did
not change, showing the OSD and the lighting change travel different
paths ([4JX issue #215 comment](https://github.com/4JX/L5P-Keyboard-RGB/issues/215)).

## 2. Reading state back from the ITE 8295

- **GET_FEATURE on report 0xCC works.** The only public code that uses
  it is maniac103's daemon:

  ```python
  def get_current_profile(self):
      return self.dev.get_feature_report(0xcc, 32)[1]
  ```

  With Python hidapi the first returned byte is the report ID, so the
  profile index sits at the same offset as the `0x16` command byte in
  the SET direction. Value 4 means off. `wait_for_profile_change()`
  polls this at 10 ms intervals, at most 100 times, after each ACPI
  event
  ([kbd_bl_api.py](https://github.com/maniac103/lenovo-kbd-backlight/blob/master/service/kbd_bl_api.py)).
- Nothing else in the returned 32 bytes has been publicly decoded. No
  repository, issue or wiki documents reading effect, speed,
  brightness or colors back. Treat "effect state is readable" as
  unverified.
- **No input report is known for Fn+Space.** The imShara payload notes,
  the origin of the `0xCC 0x16` documentation, describe writes only
  ([imShara/l5p-kbl](https://github.com/imShara/l5p-kbl)). maniac103
  polls rather than reading an interrupt endpoint, and no issue in
  4JX/L5P-Keyboard-RGB, l5p-kbl or OpenRGB mentions the device
  emitting anything. LenovoLegionToolkit identifies the RGB interface
  by a HID feature-report descriptor of length 0x21 (33 bytes, report
  ID + 32 payload) and only ever calls `HidD_SetFeature`
  ([Devices.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/System/Devices.cs),
  [RGBKeyboardBacklightController.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/Controllers/RGBKeyboardBacklightController.cs)).
- OpenRGB's `Lenovo4ZoneUSBController` (PIDs c955-c985, usage page
  0xFF89 usage 0xCC) is write-only: one `hid_send_feature_report` per
  update, no reads, no per-profile handling
  ([Lenovo4ZoneUSBController.cpp](https://gitlab.com/CalcProgrammer1/OpenRGB/-/blob/master/Controllers/LenovoControllers/Lenovo4ZoneUSBController/Lenovo4ZoneUSBController.cpp),
  [LenovoDevices4Zone.h](https://gitlab.com/CalcProgrammer1/OpenRGB/-/blob/master/Controllers/LenovoControllers/Lenovo4ZoneUSBController/LenovoDevices4Zone.h)).
- **No SELECT/SWITCH-profile HID command has been found.** Every
  public implementation sends only `0xCC 0x16`. The only known
  profile-related controls outside the HID path are the EC hotkey
  itself and the GameZone WMI method `SetLightControlOwner`, which
  LenovoLegionToolkit calls with 1/0 to take or return control of the
  lighting
  ([WMI.LenovoGameZoneData.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/System/Management/WMI.LenovoGameZoneData.cs)).

## 3. Observing Fn+Space on Linux

The chain, each link verified in a primary source:

1. EC scan matrix fires EC query `_QDE`, which executes
   `Notify (GZFD, 0xE6)` (Legion 5 15ACH6H DSDT, lines around 14697
   in the decoded dump:
   [hhd-dev/hwinfo dsdt.dsl](https://github.com/hhd-dev/hwinfo/blob/master/devices/Legion_5_15ACH6H/decoded/dsdt.dsl)).
2. `GZFD` is the GameZone WMI mapper device (`_HID` PNP0C14, `_UID`
   "GMZN"). Its `_WDG` maps notify ID 0xE6 to event GUID
   `D320289E-8FEA-41E0-86F9-811D83151B5F` (same file, `_WDG` buffer
   offset 0x190).
3. Lenovo's MOF names that GUID
   `LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT`. Present on 2021
   ([LenovoLegionLinux FEATURES_AND_TESTING.md](https://github.com/johnfanv2/LenovoLegionLinux/blob/main/doc/FEATURES_AND_TESTING.md))
   and 2022 hardware
   ([cfrstr/15ARH7 wmi-info.txt](https://github.com/cfrstr/15ARH7/blob/main/wmi-info.txt)).
   LenovoLegionToolkit subscribes to exactly this event class on
   Windows
   ([WMI.LenovoGameZoneLightProfileChangeEvent.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/System/Management/WMI.LenovoGameZoneLightProfileChangeEvent.cs)).
4. On Linux nothing consumes the GUID: mainline `lenovo/wmi-events.c`
   handles only the thermal-mode event GUID (…911D…), and
   LenovoLegionLinux binds the fan (…611D…) and other gamezone event
   GUIDs but not …811D…
   ([wmi-events.c](https://github.com/torvalds/linux/blob/master/drivers/platform/x86/lenovo/wmi-events.c),
   [legion-laptop.c](https://github.com/johnfanv2/LenovoLegionLinux/blob/main/kernel_module/legion-laptop.c)).
5. The kernel WMI core forwards every WMI event to the ACPI netlink
   socket regardless of consumers:
   `acpi_bus_generate_netlink_event("wmi", acpi_dev_name(...), *event, 0)`
   ([drivers/platform/wmi/core.c](https://github.com/torvalds/linux/blob/master/drivers/platform/wmi/core.c),
   the same call exists back through
   [v5.15 wmi.c](https://github.com/torvalds/linux/blob/v5.15/drivers/platform/x86/wmi.c)).
6. maniac103's daemon consumes it in userspace: it filters ACPI
   netlink messages for `bus_id == b'PNP0C14:01'` and
   `type == 58880` (0xE600), then polls the profile byte
   ([service/__init__.py lines 56-64](https://github.com/maniac103/lenovo-kbd-backlight/blob/master/service/__init__.py)).
   The 0xE600 constant is his observed raw value; given the DSDT it
   is the 0xE6 notify ID as packed by the netlink event struct.

What Fn+Space is not, on Linux:

- Not an evdev key. The ideapad-laptop WMI hotkey keymap covers Fn+R,
  Fn+Q and others but has no keyboard-backlight entry
  ([ideapad-laptop.c keymap](https://github.com/torvalds/linux/blob/master/drivers/platform/x86/lenovo/ideapad-laptop.c)).
  4JX states the Fn layer is invisible to applications on Windows too
  ([issue #167 comment](https://github.com/4JX/L5P-Keyboard-RGB/issues/167)).
- Not (as far as anyone has published) an input report from
  048d:c103 "ITE Device(8910)". That second ITE device exists on
  Legions as the special-keys controller
  ([linux-hardware.org entry](https://linux-hardware.org/?id=usb:048d-c103))
  but no project ties it to Fn+Space; on units where it is the only
  ITE device there is no 4-zone RGB to control.
- On white-backlight IdeaPad/Legion models the picture differs: the
  EC raises a VPC event and ideapad-laptop emits
  `brightness_hw_changed` on `platform::kbd_backlight`
  ([ideapad_acpi_notify in ideapad-laptop.c](https://github.com/torvalds/linux/blob/master/drivers/platform/x86/lenovo/ideapad-laptop.c)).
  Nothing equivalent exists in-kernel for the 4-zone RGB state.

## 4. How existing projects handle desync

| Project | Detection | Reaction |
| --- | --- | --- |
| [LenovoLegionToolkit](https://github.com/BartoszCichecki/LenovoLegionToolkit) (Windows) | WMI event subscription (`LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT`) | Owns the feature: calls `SetLightControlOwner(1)`, advances its own preset list, re-sends the full `0xCC 0x16` report ([RGBKeyboardBacklightListener.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/Listeners/RGBKeyboardBacklightListener.cs)). Re-takes ownership and re-applies the preset on resume ([PowerStateListener.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/Listeners/PowerStateListener.cs)), releases ownership on exit ([App.xaml.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.WPF/App.xaml.cs)). Refuses to run alongside Vantage. |
| [maniac103/lenovo-kbd-backlight](https://github.com/maniac103/lenovo-kbd-backlight) (Linux) | ACPI netlink event, then bounded GET_FEATURE polling | Re-applies its configured effect, mapping the hotkey to on/off; "only one profile is supported" by design ([README](https://github.com/maniac103/lenovo-kbd-backlight/blob/master/README.md)). |
| [4JX/L5P-Keyboard-RGB](https://github.com/4JX/L5P-Keyboard-RGB) | None | No Fn+Space integration; hardware profile switches silently win until the app writes again (continuously-animated custom effects overwrite on the next frame; static ones do not). Workaround offered is the app's own OS-level shortcut (Meta+RightAlt) to cycle app profiles ([issue #167](https://github.com/4JX/L5P-Keyboard-RGB/issues/167), [issue #252](https://github.com/4JX/L5P-Keyboard-RGB/issues/252)). State restore is manual autostart ([issue #106](https://github.com/4JX/L5P-Keyboard-RGB/issues/106)). |
| [OpenRGB](https://gitlab.com/CalcProgrammer1/OpenRGB) | None | Direct-mode writes only; no read, no event handling ([Lenovo4ZoneUSBController.cpp](https://gitlab.com/CalcProgrammer1/OpenRGB/-/blob/master/Controllers/LenovoControllers/Lenovo4ZoneUSBController/Lenovo4ZoneUSBController.cpp)). |

Persistence across reboot is messy and worth flagging: one 4JX user
reports colors reverting to a default after every reboot
([issue #106](https://github.com/4JX/L5P-Keyboard-RGB/issues/106)),
while another had a stuck effect surviving into BIOS
([issue #52](https://github.com/4JX/L5P-Keyboard-RGB/issues/52)).
A consistent reading (inference): a `0xCC 0x16` write changes the live
state, but at boot the EC re-applies whatever its stored current
profile holds, so software-set state appears to reset unless the tool
reapplies at login. Which write, if any, updates the stored per-profile
slots is unknown.

## Implications for Aurora

Aurora already sends the same 33-byte `send_feature_report` payload the
above tools use. The research adds three capabilities worth designing
for and one dead end:

1. **Cheap sync check: `get_feature_report(0xCC)`.** hidapi's Rust
   crate exposes it. Reading one byte tells the daemon the EC profile
   index and, critically, whether the user turned the backlight off
   (value 4). This is the only confirmed readback; do not assume
   effect or colors are readable until someone decodes the other
   bytes. A conservative design treats any counter change as "hardware
   took over, re-apply or adapt".
2. **Fn+Space detection on Linux is an ACPI netlink subscription, not
   a HID or evdev feature.** Listen on the ACPI generic-netlink event
   family for class "wmi" events from the GameZone PNP0C14 device
   (maniac103's filter: bus_id "PNP0C14:01", type 0xE600 on his 2021
   model; the underlying notify ID 0xE6 and event GUID …811D… are the
   stable identifiers). This fits the daemon-core-owns-state rule: the
   netlink fd joins the daemon's existing event loop as a blocking
   wait, no polling. After the event, the EC counter does not update
   instantly; maniac103 bounds the wait at 100 reads x 10 ms, which
   matches Aurora's named-constant bounded-retry rule.
3. **Choose a policy, because the EC will fight the daemon.** The two
   proven policies are LenovoLegionToolkit's "own the hotkey"
   (re-apply your own next-preset on every event, re-apply on resume)
   and maniac103's "collapse to on/off" (re-apply the single
   configured state, honoring off). Both re-send the full payload;
   neither trusts the hardware profiles. Windows-side ownership uses
   the GameZone WMI method `SetLightControlOwner`; no Linux driver
   exposes that method today, so on Linux the EC's native cycling
   always runs and re-applying after the event is the only available
   strategy. Expect a visible flicker of the hardware profile before
   the daemon overwrites it (maniac103's flow has the same window).
4. **Dead end: per-profile programming.** There is no known HID
   command to select a profile or write a specific profile slot, and
   no published capture of Vantage doing so. Do not build Aurora
   features that depend on programming the onboard profiles until
   someone captures Vantage traffic proving it is possible.

Also carry over the operational cautions: a wedged controller can
persist garbage state across reboots until an EC reset (NOVO button)
clears it ([4JX issue #52](https://github.com/4JX/L5P-Keyboard-RGB/issues/52)),
so Aurora should never assume a write succeeded visually; and the
daemon should re-apply saved state at startup and after resume, since
boot-time EC restoration will otherwise override it
([4JX issue #106](https://github.com/4JX/L5P-Keyboard-RGB/issues/106),
[LLT PowerStateListener.cs](https://github.com/BartoszCichecki/LenovoLegionToolkit/blob/master/LenovoLegionToolkit.Lib/Listeners/PowerStateListener.cs)).
