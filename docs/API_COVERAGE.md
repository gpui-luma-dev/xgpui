# Public API exercise coverage

This is an index of consumer exercises and their scope, not a claim that every
public symbol is covered. See [API_POLICY.md](API_POLICY.md) for compatibility
guidance and each lab's README for commands and backend-specific evidence.

| Capability | Consumer / evidence | Limits |
| --- | --- | --- |
| Window creation/closure and entity ownership | [Lifecycle lab](../apps/lifecycle-lab/README.md): shared implementation and five deterministic headless tests cover close callbacks, release, strong/weak handles, notifications, subscriptions, and actions. | macOS CI builds the native executable; mock tests do not validate native menus, OS close controls, or process exit. Native manual procedure remains pending. |
| Keyboard/action routing and focus | Lifecycle lab checks focused checkpoint/new-window actions using mock keystrokes. | Focus traversal, conflicting contexts, and native menu dispatch are not established. |
| Text and paragraph editing | [Text lab](../apps/text-lab/README.md): native/Parley comparison, wrapping, mixed scripts, selection, clipboard, composition, and virtualized scrolling. | See its tests and the [Parley guide](extensions/parley-text.md) for supported subsets and platform/IME validation limits. |
| Extension paint preparation and composition | [Paint lab](../apps/paint-lab/README.md): backgrounds, borders, paths, gradients, meshes, portable shaders, and backend checks. | Acceptance depends on the exercised adapter; compile checks alone do not establish GPU pixel behavior. |
| Wide SDR presentation | [Gamut lab](../apps/gamut-lab/README.md) and [presentation guide](extensions/high-gamut-windows.md): typed P3 producers and native composition. | Physical display fidelity, display transitions, and platform coverage require their own checks. |
| Browser paint/text integration | [Extension browser examples](../apps/extensions-web/README.md): WebGPU/WebGL2 paint gallery and shared Parley paragraph editor. | Browser GPU, clipboard, and IME acceptance remains interactive; the lifecycle lab's multi-window native contract does not apply. |
| Performance workloads | [Quad bench](../apps/quad-bench/) and [color-field bench](../apps/color-field-bench/). | Results describe the selected workloads and hardware, not all applications. |

The lifecycle lab protects general application behavior independently of the
rendering/text exercises. Remaining gaps include async ownership/cancellation,
reentrant event ordering, broad focus traversal, dialogs/clipboard platform
services, and native lifecycle checks across operating systems.
