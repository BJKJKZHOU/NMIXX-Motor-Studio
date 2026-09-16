# Encoder Page

## Purpose

The Encoder page configures the feedback interface and establishes the coordinate relationships required for servo control. It is not primarily a device-model selector and it is not a telemetry dashboard.

The page owns the human workflow around:

1. feedback interface configuration;
2. phase alignment;
3. user mechanical direction;
4. mechanical zero and optional homing.

Live motor values such as current, speed and position belong to the application-wide status area rather than being repeated on this page. Encoder health is also not shown as a permanent Ready/Valid/Fault table: normal feedback is quiet, while invalid feedback is surfaced as an application-level problem.

## Layout

The page uses the same compact engineering-tool language as Motor and Limits / Safety: a page toolbar followed by a bounded work area. The content is split approximately 3/5 to 2/5.

```text
ENCODER                                             Refresh

+--------------------------------------+--------------------------+
| Feedback Interface                   | Mechanical Reference     |
|                                      |                          |
| Interface       [ ABZ v ]            | Zero reference  Not set  |
| PPR             [ 2500 ]             |                          |
| Counts/rev        10000              | [ Set Current as Zero ]  |
|                                      | [ Software Homing ]      |
| Phase Alignment                      |                          |
|                                      |                          |
| Search current   [ 0.500 ] A         |                          |
| Phase search     [ Start ] Success   |                          |
|                                      |                          |
| Motor direction  [ Normal v ]        |                          |
+--------------------------------------+--------------------------+
```

Do not turn the sections into large dashboard cards. Their grouping comes from whitespace, headings and compact parameter/action rows.

## Feedback Interface

The first-level concept is the feedback interface/protocol, not the encoder chip model.

Examples of protocol-oriented interfaces are:

```text
ABZ
SPI Absolute
BiSS-C
SSI
...
```

Only interfaces actually supported by the firmware schema should be offered.

Protocol-specific fields appear dynamically beneath the interface selector. They should include only configuration that a normal user needs to supply; fixed board/driver details remain firmware implementation details.

### ABZ

ABZ should normally require only the encoder PPR in the first implementation:

```text
Interface        ABZ
PPR              [ 2500 ]
Counts/rev       10000
```

`Counts/rev` is a read-only derived presentation value. With fixed x4 quadrature decoding:

```text
CountsPerRev = PPR * 4
```

Do not create a second editable CPR value when it is deterministically derived from PPR.

### Current firmware mismatch

The current AxDr_L firmware exposes `PARAM_ENCODER_TYPE` as concrete driver models such as MT6816 and MT6835. That is not the intended host-facing abstraction for this page.

The intended firmware direction is to expose an interface/protocol concept plus protocol-specific parameters. Concrete chip/driver selection should remain below that abstraction unless incompatible wire protocols genuinely require a second-level protocol choice.

Until that exists, the GUI must not rename a chip-model selector to `Interface` and pretend the abstraction is already available.

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

For example, the current firmware starts phase search by selecting `PHASE_SEARCH` motor mode and then using the generic motor run action. The Encoder page must not directly encode a sequence such as:

```text
write motor mode
motor.enable
motor.run
wait for completion
```

That sequence belongs behind a semantic Application API operation such as:

```text
phaseSearch.start()
```

so GUI, CLI and automation share identical state checks, task ownership, completion and error semantics.

Until a semantic Application API operation exists, the GUI may reserve the control in a disabled/unavailable state rather than bypassing the application architecture.

## Initial implementation boundary

The first GUI implementation may wire parameters that already have the correct business semantics, including:

- phase-search current;
- user motor direction.

It should reserve but not fake functionality whose firmware/application contract does not yet exist, including:

- protocol-oriented feedback interface selection;
- ABZ PPR and derived counts/rev when ABZ is not yet exposed;
- semantic phase-search Application Action;
- zero-valid state;
- Set Current as Zero Action;
- Software Homing Action.

This allows the page layout to stabilize without coupling it to temporary firmware abstractions.