# no-angle-to-call

Small Rust CLI that converts G-code circular moves (`G2` and `G3`) into short linear `G1` moves. This is useful when the sender or controller applies a PCB height map to linear moves but not to arcs.

The converter supports `I/J/K` center offsets and `R` radius arcs, planes `G17/G18/G19`, absolute and relative endpoints (`G90/G91`), and absolute or relative arc centers (`G90.1/G91.1`). Each arc is split with a maximum step angle of 2° and a maximum chord error of 0.01 G-code units.

## Use

Check what will be converted without creating a file:

```sh
no-angle-to-call --check board.nc
```

Convert the file:

```sh
no-angle-to-call board.nc
```

The original stays untouched. The converted file is written beside it as `board.no-angle-to-call.nc`. The CLI prints the number of arcs converted and the number of resulting `G1` segments.

## Install

### Download a prebuilt binary

GitHub Actions builds binaries for Windows x86_64, Linux x86_64, macOS Intel, and macOS Apple Silicon on every push and pull request. Download the matching artifact from the workflow run. A version tag such as `v0.1.0` creates a GitHub Release with the same archives attached.

On macOS or Linux, extract the `.tar.gz` archive, then run:

```sh
chmod +x no-angle-to-call
./no-angle-to-call --help
```

On Windows, extract the `.zip` archive and run `no-angle-to-call.exe` in PowerShell.

### Build from source

Install Rust, clone this repository, then run:

```sh
cargo install --path .
```

## Notes

The converter preserves the original file and comments. It does not apply a height map itself; it turns arcs into `G1` moves so the downstream sender/controller can apply its usual compensation to each segment. Review the generated G-code before sending it to a machine.

### Windows build requirements

Install Rust (MSVC toolchain), Visual Studio C++ build tools, and a Windows 10 or 11 SDK through Visual Studio Installer. Open **Developer PowerShell for VS 2022**, change to this repository, and run:

```powershell
cargo build --release --locked
.\target\release\no-angle-to-call.exe --help
.\target\release\no-angle-to-call.exe --check .\board.nc
.\target\release\no-angle-to-call.exe .\board.nc
```

The executable is `target\release\no-angle-to-call.exe`. This is a command-line application; run it from PowerShell with an input filename.

If linking fails with `cannot open input file kernel32.lib`, install or repair the Windows SDK in Visual Studio Installer. If the Rust launcher reports a missing drive, the installed toolchain binaries can be used directly by adding `%USERPROFILE%\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin` to the current terminal's PATH.
