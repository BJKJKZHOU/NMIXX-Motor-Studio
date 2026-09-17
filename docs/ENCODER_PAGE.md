# Encoder Page

## Purpose

The Encoder page configures the feedback interface and establishes the coordinate relationships required for servo control. It is not primarily a telemetry dashboard.

The page owns the human workflow around:

1. feedback protocol/interface configuration;
2. protocol-specific encoder configuration;
3. phase alignment;
4. user mechanical direction;
5. mechanical zero and optional homing.

Live motor values such as current, speed and position belong to the application-wide status area rather than being repeated on this page. Encoder health is also not shown as a permanent Ready/Valid/Fault table: normal feedback is quiet, while invalid feedback is surfaced as an application-level problem.

## Layout

The page uses the same compact engineering-tool language as Motor and Limits / Safety: a page toolbar followed by a bounded work area. The content is split approximately 3/5 to 2/5.

```text
ENCODER                                             Refresh

+--------------------------------------+--------------------------+
| Feedback Interface                   | Mechanical Reference     |
|                                      |                          |
| Encoder Protocol [ SPI v ]           | Zero reference  Not set  |
| SPI Encoder      [ MT6835 v ]        |                          |
|                                      | [ Set Current as Zero ]  |
| Phase Alignment                      | [ Software Homing ]      |
|                                      |                          |
| Search current   [ 0.500 ] A         |                          |
| Phase search     [ Start ] Success   |                          |
|                                      |                          |
| Motor direction  [ Normal v ]        |                          |
+--------------------------------------+--------------------------+
```

Protocol-specific controls appear only when the selected protocol requires them. Do not turn the sections into large dashboard cards. Their grouping comes from whitespace, headings and compact parameter/action rows.

## Feedback Interface

The first-level concept is the feedback protocol/interface, not the encoder chip model.

The current firmware/Host schema exposes this model explicitly:

```text
PARAM_ENCODER_PROTOCOL
    -> first-level protocol selection

PARAM_ENCODER_SPI_TYPE
    -> SPI encoder selection when protocol == SPI
```

The GUI therefore presents a protocol-oriented first selector such as:

```text
Encoder Protocol
    ABZ
    SPI
    ... only protocols supported by the connected firmware
```

When a protocol needs a second-level device/configuration choice, that field is shown dynamically. For SPI this is `SPI Encoder`, with choices supplied by firmware schema/capability metadata rather than hard-coded as a universal list.

Conceptually:

```text
Encoder Protocol
    |
    +-- ABZ
    |     +-- PPR / ABZ-specific configuration when exposed
    |
    +-- SPI
          +-- SPI Encoder
                MT6816
                MT6835
                ... firmware-supported choices
```

The GUI must not invent unsupported protocol fields or pretend a capability exists before the firmware exposes it.

### ABZ

ABZ should normally require only the encoder PPR in the first implementation once that parameter is exposed:

```text
Encoder Protocol  ABZ
PPR               [ 2500 ]
Counts/rev         10000
```

`Counts/rev` is a read-only derived presentation value. With fixed x4 quadrature decoding:

```text
CountsPerRev = PPR * 4
```

Do not create a second editable CPR value when it is deterministically derived from PPR.

## Global runtime feedback

The Encoder page does not permanently show:

```text
Position
Speed
Current
Ready
Valid
Fault
```

Current, speed and position belong to the application-wide status area so they remain visible while the user works on any page.

Encoder feedback health follows the same product rule:

- valid/normal feedback is quiet;
- an invalid or faulted encoder produces an application-level problem indication;
- affected global position/speed presentation should reflect that the feedback is unavailable.

The generic Parameters page remains available when an expert wants to inspect raw Ready/Valid/Fault parameters.

## Phase Alignment

Phase alignment establishes the relationship between positive motor current / internal electrical direction and encoder feedback.

The user configures only the search current and starts the operation:

```text
Phase Alignment

Search current       [ 0.500 ] A
Phase search         [ Start ]  Running / Success / Failed
Motor direction      [ Normal v ]
```

The phase-search result is automatically applied by the firmware when the operation succeeds. The Encoder page therefore does not show an Apply button.

The resulting encoder direction and electrical offset are implementation/calibration results. They remain readable through the generic Parameters page but are not permanently displayed on the normal Encoder page. For the normal workflow, `Success` is sufficient.

Phase Search follows the shared stateful-action interaction rule in `UI_INTERACTION_RULES.md`: while the same search task is active, the action control may change from Start to Abort/Stop in the same location rather than presenting two permanently adjacent controls.

### Encoder direction versus motor direction

These are different concepts.

Phase search determines the automatic calibration relationship:

```text
positive current / internal motor direction
                <->
         encoder positive direction
```

This can determine encoder direction and electrical offset, but it cannot know which physical direction the user's application should call positive.

`Motor direction` is therefore a separate user configuration. It maps the internally calibrated mechanical direction to the user's mechanical coordinate convention.

The normal UI uses human-facing values such as:

```text
Normal
Reversed
```

while firmware may continue to represent the value as `+1 / -1`.

Changing Motor direction does not require phase search to be repeated because it changes the user mechanical coordinate mapping, not the encoder-to-electrical calibration.

## Mechanical Reference

The right-hand column groups the operations that establish mechanical position zero.

For the current product scope it stays intentionally compact:

```text
Mechanical Reference

Zero reference       Not set

[ Set Current as Zero ]  [ Software Homing ]
```

### Set Current as Zero

Setting the current position as zero is an Action, not a value parameter. It establishes the current physical position as the origin of the user mechanical coordinate system.

The Application API should eventually expose a semantic operation such as:

```text
position.setZero()
```

and a read-only zero-valid state.

### Homing

Homing is different from setting the current position as zero:

```text
Set Zero
    current position -> zero
    motor does not need to search for a reference

Homing
    motor moves -> finds a reference -> establishes zero
```

The current firmware only has the software homing concept under discussion, so the first GUI should not invent digital-input, optical-switch, trigger-polarity, fast/slow approach or home-offset controls before those firmware semantics are defined.

When external homing is implemented later, the Mechanical Reference area can expand to expose the supported source and motion parameters.

## Position limits relationship

Position Limits live on Limits / Safety, not on the Encoder page, but they depend on the zero reference established here.

Conceptually:

```text
Encoder / Mechanical Reference
        -> ZeroValid

Limits / Safety
        -> PositionLimitEnable requires ZeroValid
```

Without a valid mechanical zero, position limits cannot be enabled because their minimum and maximum values are defined relative to zero.

## Application API boundary

The GUI must not reproduce motor-state orchestration required to perform phase search or homing.

The current AxDr_L firmware starts phase search by selecting `PHASE_SEARCH` motor mode, completing the generic motor-enable action, and then using the generic motor-run action. That device-side composition is hidden by the Rust Application API:

```text
GUI / CLI / Automation
        |
        v
phaseSearch.start()
        |
        v
MotorActionService
        |
        +-- PARAM_MOTOR_MODE = PHASE_SEARCH
        +-- ACTION_MOTOR_ENABLE
        +-- wait for Enable completion
        +-- ACTION_MOTOR_RUN
        +-- expose final completion as semantic phase-search completion
```

The desktop client calls the semantic phase-search command. It does not know or reproduce the firmware sequence.

This is intentionally an Application-level compatibility adapter. If firmware later exposes a native phase-search Action, only the Application implementation needs to change; Encoder GUI, CLI and automation semantics stay stable.

The same rule applies to future Set Zero and Homing operations: clients call semantic Application operations, while any required lower-level Parameter/Action composition remains inside the Application layer.

## Initial implementation boundary

The current GUI wires parameters/actions that already have a stable business meaning:

- `Encoder Protocol` through `PARAM_ENCODER_PROTOCOL`;
- SPI encoder selection through `PARAM_ENCODER_SPI_TYPE` when SPI is selected;
- phase-search current;
- semantic phase search through `MotorActionService`;
- user motor direction.

It reserves but does not fake functionality whose firmware/application contract does not yet exist:

- ABZ PPR and derived counts/rev until ABZ-specific configuration is exposed;
- zero-valid state;
- Set Current as Zero Action;
- Software Homing Action.

This allows the page layout to stabilize without coupling it to temporary firmware abstractions.
