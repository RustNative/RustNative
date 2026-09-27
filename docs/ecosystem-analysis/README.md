# Ecosystem analysis

A standing competitive analysis of the application frameworks RustNative is
measured against, and the plan for reaching or exceeding their production bar.

## Why it exists

RustNative's architecture is decided (`PLAN.md` sections 1–2). What this
analysis adds is the other half of production readiness: the accumulated
answers mature frameworks give to questions an architecture document does not —
how a developer debugs a live tree, how an application is updated after it
ships, what happens on the third day of a migration, what a security reviewer
asks for, what a translator is handed.

Those answers are where frameworks are won and lost. A framework with a better
core and no inspector loses to a worse core with one.

## It is ordered from the root up

A framework is a stack of decisions, and the decisions at the bottom constrain
everything above them. A framework that chose a garbage-collected runtime at
layer 0 cannot have deterministic destruction at layer 6. A framework that
chose to draw its own widgets at layer 2 cannot inherit the host's
accessibility at layer 5.

So the analysis starts at the substrate and climbs — language and memory model,
execution and loop ownership, what a widget physically is, identity and
layout and text, the application model, host integration, services, the
engineering loop, shipping, and finally the project around it. A weakness at
layer 7 is a backlog item; a weakness at layer 1 is permanent, and only the
bottom-up order makes the difference visible.

Read in this order:

1. [`method-and-stack.md`](method-and-stack.md) — the ten layers, the parity
   bar per layer, the scoring rules, and the four asymmetries.
2. [`foundations.md`](foundations.md) — the root strategies themselves
   (substrate, execution, realization, identity, layout, text, state, effects,
   accessibility, styling, and the engineering loop), each with the ceiling it
   imposes on everything above it. **The most important document here.**
3. The platform documents — concrete archetypes analysed as paths through those
   root choices: [`web.md`](web.md), [`desktop.md`](desktop.md),
   [`mobile.md`](mobile.md), [`embedded.md`](embedded.md). Each archetype
   ends by naming the concepts it introduced.
4. The concept catalogue — the ninety-odd *ideas* those archetypes introduced,
   analysed on their own merits, because a concept can outlive the archetype
   that carried it and become an expectation everywhere:
   [`concepts-core.md`](concepts-core.md) (scheduling, rendering model, state,
   composition), [`concepts-app.md`](concepts-app.md) (UI system, data and
   sync, server and API, distributed execution, platform surfaces),
   [`concepts-delivery.md`](concepts-delivery.md) (engineering loop, build and
   packaging, isolation and security, ecosystem, and the concepts deliberately
   rejected), [`concepts-embedded.md`](concepts-embedded.md) (hardware,
   scheduling, memory, storage, fleets, robotics, edge inference), and
   [`concepts-toolchain.md`](concepts-toolchain.md) (the layer between a
   repository and an installed, updatable application: release trains,
   diagnostics, profiles and credentials, submission, update channels and
   compatibility fingerprinting, pipelines, native-module authoring, universal
   routing, launch assets, playground, device pairing, the escape ladder, and
   artifact attribution), [`concepts-surface.md`](concepts-surface.md) (the
   frame pipeline, scroll and gesture systems, the text editing surface, media,
   capture, and files), [`concepts-product.md`](concepts-product.md) (the
   obligations stores and regulators impose, and what a product team needs
   after the application works),
   [`concepts-discipline.md`](concepts-discipline.md) (the practices that keep
   the framework's own promises true), and
   [`concepts-reach.md`](concepts-reach.md) (managed fleets, desktop shells,
   constrained networks, the browser's changing rules, and the field).
5. [`parity-matrix.md`](parity-matrix.md) — where RustNative stands on every
   layer and every concept today, with evidence.
6. [`gap-plan.md`](gap-plan.md) — the workstreams and proposed milestones that
   close the delta, tiered and ordered, plus the differentiation statement.

## The naming rule

**No framework, product, vendor, or library is named anywhere in this
directory, and none should be introduced later.**

Every competitor is described as an *archetype*: a family of tools sharing a
strategy, with the strategy stated in mechanism terms. Naming products would
anchor future work on imitation — copying an API surface, inheriting a mistake,
or treating someone's roadmap as ours. Archetypes force the analysis down to
the mechanism, which is the only part that transfers.

Operating-system APIs, protocols, and standards *are* named: they are the hosts
RustNative realizes onto, not competitors (`PLAN.md` 2.2). Technique names —
retained and immediate mode, islands, resumability, fine-grained reactivity,
tree diffing, measure/arrange — are used freely, because they name mechanisms
rather than vendors.

## How each archetype is treated

Bottom-up, in the same order, every time: root (L0–L2), semantics and model
(L3–L4), integration (L5–L6), loop and ship (L7–L8), project (L9), then
strengths, weaknesses, opportunities, and threats *derived from that climb* —
and finally falsifiable requirements, tagged `[X]` cross-cutting, `[W]` web,
`[D]` desktop, `[M]` mobile, `[E]` embedded/RTOS, `[T]` terminal, each written
so a milestone can be derived and a test can prove it.

Requirement identifiers are stable. [`parity-matrix.md`](parity-matrix.md)
scores against them and [`gap-plan.md`](gap-plan.md) schedules them.

## What this analysis does not do

It does not propose adopting another framework's architecture. `PLAN.md`
section 2 is the architecture, and nothing here overrides it. Where an
archetype's strength depends on a decision we deliberately made differently,
the analysis says so and extracts the *outcome* rather than the mechanism.

It also does not treat parity as the goal. Parity is the price of being
considered; the argument for being chosen is the differentiation statement at
the end of [`gap-plan.md`](gap-plan.md), and it should be re-derived, not
assumed, whenever this analysis is revisited.
