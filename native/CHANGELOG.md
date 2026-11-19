# Changelog (native fork)

Versions here are this fork's release line. The upstream service version the
fork is synced to is recorded in `tools/parity/parity.toml`.

## 1.4.1

- **Install update** works again, and you can see it working. The app now
  downloads the new installer itself and runs it with Windows' progress
  window (and the administrator prompt), then reopens when it's done. If an
  update doesn't finish, the app tells you the next time it opens and offers
  **Try again** and **Open the log**.
- Background automatic updates now also run on battery. They are run by a
  Windows scheduled task, and Windows' default for those is "start only on AC
  power", so on battery the installer never started.
- The keyboard app is found under its new name,
  `framework-16-keyboard-control.exe` (the old `f16-lights.exe` still works).

## 1.4.0

- **Your tools** at the top of the Overview: one-click cards to open White
  Rabbit and Framework 16 Keyboard Control. Only tools that are installed
  (or that you pointed to with *Browse*) are shown.
- The Tools page links to each tool's GitHub project.

## 1.3.2

- Installer: really no more "a restart will be required" warning. The
  warning came from Windows Installer's check for files in use, which ran
  while Framework Control's window was still open (the close step came too
  late), even though nothing ended up needing a restart. The installer now
  closes the app just before that check. Nothing in Framework Control needs
  a restart; if Windows ever decides one is needed, its own end-of-install
  prompt offers to restart now or later.

## 1.3.1

- Fixed **Install update** in Settings: the update never finished. The
  service started the installer as its own child process, and when the
  installer stopped the service to replace its files, the installer was
  ended along with it and the update rolled back. The installer now runs on
  its own (through Windows Task Scheduler) and writes a log to
  `%ProgramData%\FrameworkControl\update-install.log`.
- After you click **Install update**, Framework Control now closes and
  reopens by itself once the update is done.
- **To get this fix from 1.3.0 or earlier**, download and run the 1.3.1
  installer once by hand; in-app updates work from then on.

## 1.3.0

- New look that matches **White Rabbit** and the Windows 11 Settings app:
  - It follows Windows' light or dark mode and your accent colour, or you
    can pick Light or Dark.
  - A side menu with Fluent icons, large page titles, and settings laid out
    as cards with a short explanation each.
  - Windows-style on/off switches.
- The web UI's colour themes (Retro, Synthwave and so on) are no longer
  offered; if one was chosen, the app follows Windows instead.
- Installer: no more "restart required" messages. The installer now closes
  Framework Control's window before updating its files (the background
  service was already stopped), so nothing is in use and no restart is
  needed.

## 1.2.0

- The Tools page now launches **White Rabbit**, the all-in-one LED Matrix
  app, instead of the LED Matrix Monitor (led_mon). Buttons: *Open*, *Edit
  panel layout* and *Games & patterns*. White Rabbit's own Framework Control
  button opens this app, so the two open each other.
- The keyboard app is now called **Framework 16 Keyboard Control**.

## 1.1.0

- New **Tools** page that launches your other Framework 16 tools:
  - **LED Matrix Monitor** (led_mon): *Start monitor* and *Edit layout*.
  - **Framework 16 Lights** (keyboard + macropad lighting): *Open*.
- Finds each tool in its usual install location, shows whether it's running
  (led_mon can't run twice, so *Start monitor* waits until it isn't), offers a
  download link when a tool is missing, and lets you browse to a different
  copy. Your choice is remembered per user.
- The Tools page works even when the background service isn't running.
- Synced with upstream Framework Control 0.5.5.

## 1.0.0

- Native Windows app (Rust + egui) replaces the browser UI: about 5× less
  memory than Chromium app mode, no browser required.
- Full feature parity with the upstream web UI: sensors, fan profiles with
  AC/battery auto-switch, Auto/Manual/Curve modes, per-fan overrides,
  calibration with live duty markers, power profiles (RyzenAdj install on
  Windows), battery limits and history, framework_tool version switching,
  logs, themes and feature toggles.
- Single MSI installs the service and the app, with Start Menu and Desktop
  shortcuts for the app. Replaces an existing upstream install and keeps
  settings.
- In-app updates from this repository's GitHub releases (silent, no UAC).
