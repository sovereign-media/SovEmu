# Controllers

How every client (web, Android, iOS) names controllers and their buttons, so that a user's
button mapping, saved once on the server, applies wherever they play. Each client implements
this document; change it and all three clients together.

## Controller IDs

A mapping belongs to a controller **model**, not a single device. Two DualSense pads share
one mapping, and so does a DualSense on the web and the same pad on an iPhone. The ID is
lowercase ASCII (`a-z`, `0-9`, `-`, `:`), at most 64 characters:

| ID | What |
|---|---|
| `keyboard` | Any keyboard. All keyboards share one mapping. |
| `xbox` | Every Microsoft Xbox pad (360, One, Series, Elite) |
| `dualsense` | PlayStation 5 DualSense and DualSense Edge |
| `dualshock4` | PlayStation 4 DualShock 4 |
| `switch-pro` | Nintendo Switch Pro Controller |
| `pad:<vendor>:<product>` | Any other controller that reports USB/Bluetooth IDs, as four lowercase hex digits each: `pad:2dc8:6101` |
| `pad:name:<slug>` | A controller without IDs, by its name: lowercase, every run of other characters a `-`, trimmed of `-`, at most 48 characters |

The known models, and how each platform recognises them:

| ID | Vendor:product (web, Android) | iOS `GCController.productCategory` | Other names (web) |
|---|---|---|---|
| `xbox` | `045e:` `028e` `0291` `0719` `02d1` `02dd` `02e0` `02e3` `02ea` `02fd` `0b00` `0b05` `0b12` `0b13` `0b20` `0b22` | `Xbox One` | contains `xinput` or `xbox` |
| `dualsense` | `054c:0ce6` `054c:0df2` | `DualSense`, `DualSense Edge` | contains `dualsense` |
| `dualshock4` | `054c:05c4` `054c:09cc` `054c:0ba0` | `DualShock 4` | contains `dualshock 4` or `wireless controller` with vendor `054c` |
| `switch-pro` | `057e:2009` | `Switch Pro Controller` | contains `pro controller` with vendor `057e` |

Finding the IDs:

- **Web**: the Gamepad API's `gamepad.id` carries them, worded per browser:
  - Chrome: `Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)`;
  - Firefox: `045e-0b13-Xbox Wireless Controller`;
  - Safari: `45e-b13-Xbox Wireless Controller` (not zero-padded).
  Chrome on Windows reports XInput pads as `Xbox 360 Controller (XInput STANDARD GAMEPAD)`, with no IDs, so they are `xbox`. The same applies whether the pad is on USB or Bluetooth.
- **Android**: `InputDevice.vendorId` / `productId` (USB and Bluetooth); `pad:name:` from `InputDevice.name` when both are 0.
- **iOS**: `GCController.productCategory` gives the known models. Anything else is `pad:name:` from `vendorName` (an MFi pad: `pad:name:<vendorName>`).

## Families

The family picks the button labels a client shows. It comes from the model, otherwise from the vendor (`045e` Xbox, `054c` PlayStation, `057e` Nintendo), otherwise from the name, and otherwise it is `generic`, which uses the Xbox labels.

| Input | Xbox / generic | PlayStation | Nintendo |
|---|---|---|---|
| `south` | A | ✕ | B |
| `east` | B | ○ | A |
| `west` | X | □ | Y |
| `north` | Y | △ | X |
| `l1` `r1` | LB RB | L1 R1 | L R |
| `l2` `r2` | LT RT | L2 R2 | ZL ZR |
| `select` `start` | View Menu | Create Options | − + |
| `l3` `r3` | LS RS | L3 R3 | LS RS |
| `up` `down` `left` `right` | D-pad | D-pad | D-pad |

## Inputs

A controller's inputs are named **by position**: `south east west north l1 r1 l2 r2 select
start l3 r3 up down left right`. This follows the W3C standard gamepad's buttons 0 to 15, in that order. A
client maps its platform's own names onto these:

- **Web**: the standard mapping's button index *n* is the *n*th name above.
- **Android**: `BUTTON_A` south, `BUTTON_B` east, `BUTTON_X` west, `BUTTON_Y` north, `L1`, `R1`, `L2`, `R2` (and the trigger axes), `SELECT`, `START`, `THUMBL`, `THUMBR`, `DPAD_*` (and the hat axes).
- **iOS**: `GCExtendedGamepad` `buttonA` south, `buttonB` east, `buttonX` west, `buttonY` north, the shoulders, triggers, `buttonOptions` select, `buttonMenu` start, the thumbstick buttons and the D-pad.

A controller's **stick directions** are inputs too: `ls_up ls_down ls_left ls_right` (the left
stick) and `rs_up rs_down rs_left rs_right` (the right), each pressed while that stick is pushed past
half-way that way. They are bound like buttons; none is bound by default, and a diagram lights the
stick (`l3`, `r3`) for them. Families label them `LS ↑` … `RS →`.

The guide/home button is the system's and is never bound.

A keyboard's inputs are W3C `KeyboardEvent.code` values: `KeyZ`, `ArrowUp`, `Enter`,
`ShiftLeft`, `Digit1`, `Space`… On Android and iOS a client maps the platform's key codes onto
these names. `Tab` and `Backspace` are the fast-forward and rewind keys and cannot be bound.

## Mappings

A mapping is a list of `{input, button}` pairs. Each input drives one RetroPad button (`B Y SELECT START UP DOWN LEFT RIGHT A X L R L2 R2 L3 R3`), and a button may have several inputs. A mapping **replaces** the built-in defaults for that controller.

Consoles label their buttons differently: the Mega Drive's A, B and C are RetroPad Y, B and A. So a mapping is for a controller model **on one system**, or on **every system** (`system` null, the mapping that applies wherever there is no system's own). The mapping in force for a user playing a system is the first of:

1. **their own** for that system;
2. **their own** for every system;
3. **the default** for that system;
4. **the default** for every system;
5. **the built-in defaults** below.

The user's own choices come before any default. A default comes from the environment, or from an admin on the admin page (Admin ▸ Controllers). For the same controller and system, the environment wins, and the admin page shows that default locked. The environment variables are `SOVEMU_CONTROLLERS__<n>__ID`, `__BINDINGS` (`south:B,east:A,…`), and optionally `__SYSTEM` and `__NAME`. The admin page offers its own defaults in that form.

`controllerMappings(system:)` returns the mapping in force for each model that has one on that system. Each comes with its `source` (`USER`, `ADMIN` or `ENVIRONMENT`) and its `system` (null when it is an every-system mapping). Without `system`, it returns the every-system mappings. A model missing from the list uses the built-in defaults. After a reset, a client fetches the list again, so the next layer applies. The built-in defaults:

- **Controllers**: by position, as libretro's SNES-shaped RetroPad expects: `south` B, `east` A, `west` Y, `north` X, `l1` L, `r1` R, `l2` L2, `r2` R2, `select` SELECT, `start` START, `l3` L3, `r3` R3, and the D-pad to the D-pad.
- **Keyboard**: arrows to the D-pad, `KeyZ` B, `KeyX` A, `KeyA` Y, `KeyS` X, `KeyQ` L, `KeyW` R, `Enter` START, `ShiftLeft` and `ShiftRight` SELECT.

The server stores one mapping per user, controller ID and system (or every system), and one default per controller ID and system. The GraphQL operations are `controllerMappings`, `saveControllerMapping` and `resetControllerMapping`, each with an optional `system`; for admins, `controllerDefaults`, `saveControllerDefault` and `resetControllerDefault`. The server checks the IDs, systems and input names above, in the API and in the environment.

## Systems and diagrams

Clients show each system's own labels, and side-by-side diagrams. The server gives both, so that every client draws the same thing:

- **`System.controls.buttons`**: the RetroPad buttons the system's pad has, in order, each with the system's label (Mega Drive: `Y` → `A`, `B` → `B`, `A` → `C` …). A client lists these when remapping for that system; for every system, it lists all sixteen by RetroPad name.
- **`System.controls.sticks`**: the analog controls the system's core reads from the player's sticks (N64: `LEFT` → Control stick, `RIGHT` → C buttons; GameCube, Dreamcast and PS2 likewise), each with the console's label and the `key` it lights on the system's diagram (`LS`, `RS`, or a button's key such as `L3`). Sticks are not remapped: the left stick always drives the left analog control and the right stick the right one. A client lists them after the buttons, for that system only, as rows that show their stick (`Left stick`, `Right stick`) and cannot be picked to rebind; focusing one lights its key on the system's diagram and the stick (`l3`, `r3`) on the controller's.
- **`System.controls.layout`**: the system's pad, whose keys are RetroPad buttons (`B`, `START` …) labelled as the console prints them, plus the keys of its sticks.
- **`controllerLayouts`**: the player's controllers by family (`xbox`, `playstation`, `nintendo`, `generic`), whose keys are input names (`south`, `l1` …) labelled as in Families above. The keyboard has no diagram; it shows key names.

A layout is drawn on a `width` × `height` canvas (160 × 100) from rounded rectangles, `{x, y, w, h, r}`. A shape is a circle when `r` is half its size.

- **`body`**: shapes filled in one colour, with `tone` 1 for a darker recess (a screen, a stick's well).
- **`keys`**: one shape per button, with its `label` drawn in it.

A client highlights the button being remapped on both diagrams at once: the system's key for the RetroPad button, and the controller's keys for every input mapped to it.

The drawings are SovEmu's own schematics, not any maker's artwork. The 3D systems' labels follow their cores' defaults and are the least certain.

## Remapping

A client rebinds a button by waiting for the next input on that one device:

- **Any other new input** binds at once.
- **The client's menu Back button** (Escape on a keyboard): a quick tap cancels, and holding it for 1 second binds it. That way Back can be bound too.
- **Home/guide** cancels.
- **8 seconds** with no new input cancels.
- **Inputs already held** when the wait starts are ignored until they are released.

The prompt says so, for example: "Press a button for B · tap Back to cancel, hold it to bind it".

## In play

- **Hotkeys** are read after mapping, from the RetroPad buttons: SELECT+R fast-forwards, SELECT+L rewinds, and SELECT+START opens the menu. Those buttons are kept from the game while a combo is held.
- **Sticks**: the left stick also drives the D-pad, unless the system reads analog sticks or the
  mapping binds any of the left stick's directions (`ls_*`): then it does what they are bound to.
  Bound stick directions apply on every system, alongside the analog values.
- **Players**: each controller is its own player, in the order controllers connect. A new one takes the lowest free player (1 to 4), and a disconnected one frees its player. The keyboard and the touch controls are player 1. A client lets the player reassign them during a session; the assignment is not saved.
