# Quest Emu Setup

This project provides tools for setting up a Quest-like emulator and patching APK files to run within it.

```shell
Usage: quest_emu_setup [OPTIONS] <COMMAND>

Commands:
  setup  
  apk    
  help   Print this message or the help of the given subcommand(s)

Options:
      --yes   Skip all prompts and use default values
  -h, --help  Print help
```

## Cargo features

Each device backend is a feature. Both are on by default, and at least one is required:

| Feature    | Default | Enables |
|------------|---------|---------|
| `avd`      | yes     | The Android SDK emulator backend (`--backend emulator`): SDK/emulator/system image `setup avd`, `create`, and running AVDs with `start avd`/`stop` |
| `waydroid` | yes     | The Waydroid backend (`--backend waydroid`). Waydroid itself only runs on Linux |

The adb-based commands (`apk install`, `debug`) work with either backend, as
does APK `download`/`patch`.

## Device backends

`quest_emu` can manage two kinds of device. `setup` and `start` take the backend
as a subcommand:

- `avd`: the Android SDK emulator (AVDs). Uses `setup avd` → `create` → `start avd`.
- `waydroid`: a [Waydroid](https://waydro.id) container, reached over TCP adb.
  Uses `setup waydroid` → `start waydroid`, since it has no AVDs.

`create` makes AVDs, so it is emulator-only. `stop`, `apk install` and `debug`
take `--backend emulator|waydroid`. If it's omitted, they use whatever is running
(Waydroid first, then an emulator), then the backend last used by `setup` or
`start`, then whatever is installed.

```shell
quest_emu setup waydroid             # checks install, `waydroid init` and ARM translation
quest_emu start waydroid             # container + session + UI, then adb connect
quest_emu stop  --backend waydroid   # waydroid session stop
```

`setup waydroid` checks that Waydroid is installed and initialized
(`sudo waydroid init -s VANILLA`), and that ARM translation (libndk_translation or
libhoudini) is configured. Quest apps are arm64-only and won't install without it.
`quest_emu` never runs anything as root. When a step is missing, it stops and
prints the command to run, for ARM translation typically via
[waydroid_script](https://github.com/casualsnek/waydroid_script). Likewise,
`start waydroid` asks you to `sudo systemctl start waydroid-container` if the
container service isn't running.

`start waydroid` opens the Waydroid UI on purpose. While no UI is
open, Waydroid freezes the container, and adb then reports the device as `offline`.

### Picking the adb device

`apk` and `debug` talk to exactly one adb device:

1. `-s/--serial <serial>` (TCP serials like `192.168.240.112:5555` are
   `adb connect`ed automatically)
2. `$ANDROID_SERIAL`
3. `--backend waydroid` / `--backend emulator`
4. otherwise the only online device, or a running Waydroid session

If several devices are connected and none was picked, the command errors instead of guessing.

```shell
quest_emu apk install ./apk/1.44.1_20239                   # folder with one .apk + its .obb files
quest_emu apk install --backend waydroid ./beatsaber.apk
```

`apk install` reads the package name from the APK's manifest, so the file can
have any name. It never deletes the input folder. `apk download --install --cleanup`
deletes the downloaded version folder after a successful install.

## Debugging native code (lldb-dap)

The app must be debuggable (`android:debuggable="true"`), since lldb-server
is copied in with `run-as`. The lldb-server arch is taken from the installed
app's ABI, not the device's. On an x86_64 device with ARM translation
(Waydroid, x86_64 emulator images), an arm64 app resolves to arm64.

```shell
quest_emu debug install <package> [arch]   # push + copy lldb-server into the app's private storage
quest_emu debug attach <package>           # install + start lldb-server + attach info, in one go
```

`attach` installs lldb-server into the app's private storage, starts it as a
platform server, forwards the debug port, and by default attaches to the app
as it's already running — pass `--relaunch` to force-stop and relaunch it
fresh first (useful right after installing/reinstalling the app, or when you
want a clean process). It prints the 4 lines needed to actually connect a
debugger once the target process is found:

```
Port: 5039
Attach URI: vscode://llvm-vs-code-extensions.lldb-dap/start?config=...
PID: 12345
Config: {"request":"attach","pid":12345,"attachCommands":[...]}
```

### Example: debugging a QPM mod

If you're iterating on a [QPM](https://github.com/QuestPackageManager/QPM.CLI)
mod, build it first so `quest_emu` has a `.so` with debug symbols to load, then
attach with that build folder as `--symbols` (any `.so` files under it are
picked up automatically):

```shell
qpm build   # or ./build.sh / build.ps1, produces ./build/libmymod.so

quest_emu debug attach com.beatgames.beatsaber --relaunch --symbols ./build --open
```

`--open` opens the printed `Attach URI` in VS Code directly (requires the
[llvm-vs-code-extensions.lldb-dap](https://marketplace.visualstudio.com/items?itemName=llvm-vs-code-extensions.lldb-dap)
extension) and starts debugging immediately, no `launch.json` needed.

### Example: `tasks.json` + `launch.json`, fully automatic with F5

For a reusable debug entry instead of `--open`, use `quest_emu` as a
`preLaunchTask` that writes the fresh pid to a file, and read it back into
`launch.json` with the [Command Variable](https://marketplace.visualstudio.com/items?itemName=rioj7.command-variable)
extension:

`.vscode/tasks.json`:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "quest-emu: attach",
      "type": "shell",
      "command": "quest_emu",
      "args": [
        "debug", "attach", "com.beatgames.beatsaber",
        "--relaunch",
        "--symbols", "./build",
        "--pid-file", "${workspaceFolder}/.vscode/.quest-pid"
      ],
      "problemMatcher": [],
      "presentation": { "reveal": "always", "panel": "dedicated" }
    }
  ]
}
```

`.vscode/launch.json`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Attach to Quest mod (lldb-dap)",
      "type": "lldb-dap",
      "request": "attach",
      "preLaunchTask": "quest-emu: attach",
      "pid": "${input:questPid}",
      "attachCommands": [
        "platform select remote-android",
        "settings set target.inherit-env false",
        "platform connect connect://localhost:5039",
        "attach -p ${input:questPid}",
        "pro hand -p true -s false SIGPWR",
        "pro hand -p true -s false SIGXCPU",
        "pro hand -p true -s false SIG33",
        "target symbols add ./build/libmymod.so"
      ]
    }
  ],
  "inputs": [
    {
      "id": "questPid",
      "type": "command",
      "command": "extension.commandvariable.file.content",
      "args": { "fileName": "${workspaceFolder}/.vscode/.quest-pid" }
    }
  ]
}
```

Hitting F5 runs the task (installs/starts lldb-server, force-stops and
relaunches the app, writes its pid to `.vscode/.quest-pid`), then the
`questPid` input re-reads that file — after the task finishes, so it's always
current — and feeds it into both the `pid` field and the `attach -p` command
before lldb-dap connects. No copy-pasting between runs.

- `platform connect connect://localhost:<port>`: the port is `adb forward`ed
  to the device, so it's always `localhost`; update it if you pass `--port`.
- `target symbols add <path>`: one per `.so` under the path(s) passed to
  `--symbols` — here, QPM's `./build` output.