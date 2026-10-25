# Documentation

Start with the [project README](../README.md) for an introduction and a local standalone HTTP viewer demo. These guides are maintained with the source and do not require another documentation checkout.

| Guide | Read it for |
|---|---|
| [Publication hygiene](publication.md) | Local recordings, source allowlist, credential checks and retained attribution. |
| [SDK contract and release checks](sdk-build-plan.md) | Source-independent consumer builds, local SDK production, integrity and acceptance. |
| [Build setup](build.md) | Automatic pinned source setup, optional SDKs, prerequisites, optional comparison recordings and troubleshooting. |
| [Viewer operation](viewer.md) | Standalone configuration, stores, navigation, replay limits and live-site behavior. |
| [Architecture](architecture.md) | Runtime/API/viewer responsibilities, event ordering, navigation and hibernation. |
| [Source layout](source-layout.md) | Crates, browser adapters, test fixtures and entry points for changes. |
| [Testing](testing.md) | Browser-free checks, real-browser end-to-end tests and Firefox API comparisons. |
| [Measurement tools](measurements.md) | Memory/layout comparisons, hibernation experiments and HTML parser maintenance. |

Unless a command says otherwise, run it from the repository root. Paths in code spans refer to that root. Commands using `mach` run from a compatible Firefox source checkout.
