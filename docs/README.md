# ApexShot documentation

Reference documentation for the ApexShot desktop codebase. Install, architecture,
module, and developer docs live here; active plans live in [`plans/`](plans/).

## Using ApexShot

| Document | What it covers |
|---|---|
| [INSTALL.md](INSTALL.md) | Installing on Windows and every supported Linux distro, updating, the GNOME extension, and the Linux CLI. |
| [AGENT_INTEGRATION.md](AGENT_INTEGRATION.md) | The JSON agent CLI and MCP server for scripted captures. |

## Reference

| Document | What it covers |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module layout and how the major subsystems fit together. |
| [MODULES.md](MODULES.md) | Per-module reference for `src/`. |
| [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) | Building, running, and debugging from source. |
| [WINDOWS_EXPORT_PIPELINE.md](WINDOWS_EXPORT_PIPELINE.md) | How the Windows video editor renders and encodes an export. |
| [EXPORT_HARDWARE_ENCODING.md](EXPORT_HARDWARE_ENCODING.md) | Hardware-accelerated export paths and limits. |
| [GPU_CURSOR_COMPOSITING.md](GPU_CURSOR_COMPOSITING.md) | Cursor compositing on GPU capture paths. |
| [STUDIO_ENTITLEMENT.md](STUDIO_ENTITLEMENT.md) | The signed Studio licence format and its offline behaviour. |
| [COMMERCIAL_TRANSITION.md](COMMERCIAL_TRANSITION.md) | What stays free and what Studio adds. |
| [THIRD_PARTY_WALLPAPERS.md](THIRD_PARTY_WALLPAPERS.md) | Third-party wallpaper assets and licensing. |

## Packaging and releases

| Document | What it covers |
|---|---|
| [RELEASE_OPERATIONS.md](RELEASE_OPERATIONS.md) | Support path, pre-tag configuration, promotion, and rollback. |
| [ARCH_SUPPORT.md](ARCH_SUPPORT.md) | Arch Linux packaging and distribution support. |
| [AUR_PUBLISHING.md](AUR_PUBLISHING.md) | Release → AUR automation. |
| [CLOUDFLARE_CDN_SETUP.md](CLOUDFLARE_CDN_SETUP.md) | Cloudflare installer entrypoint and release mirror. |
| [`packaging/windows/README.md`](../packaging/windows/README.md) | Building the Windows installer. |
| [`packaging/nix/README.md`](../packaging/nix/README.md) | The Nix package. |
| [`flatpak/README.md`](../flatpak/README.md) | Building the Flatpak locally. |

## Plans

Active planning documents. Finished plans are removed rather than archived;
recover any of them from git history when needed.

| Document | What it covers |
|---|---|
| [plans/FLATHUB_LAUNCH_PLAN.md](plans/FLATHUB_LAUNCH_PLAN.md) | Flathub launch audit and remaining blockers. |
| [plans/FLATPAK_STRATEGY.md](plans/FLATPAK_STRATEGY.md) | Flatpak packaging strategy. |
| [plans/FLATPAK_TESTING.md](plans/FLATPAK_TESTING.md) | Flatpak acceptance checks. |
| [plans/FLATPAK_BROWSER_BRIDGE.md](plans/FLATPAK_BROWSER_BRIDGE.md) | Browser integration under Flatpak. |
| [plans/FLATPAK_PORTAL_ONLY_CHANGES.md](plans/FLATPAK_PORTAL_ONLY_CHANGES.md) | Portal-only Flatpak code changes. |

## Contributing

See [`CONTRIBUTING.md`](../CONTRIBUTING.md) for setup and style rules, and
[`README.md`](../README.md) for the user-facing overview.
