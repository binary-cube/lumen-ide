# ADR-0001: DDD + arhitectură hexagonală, un crate per bounded context

**Status:** Acceptat (2026-10-19)

## Context
Editorul trebuie să fie modular, cu limbaje și drivere înlocuibile, și testabil fără GPU sau sistem de fișiere real.

## Decizie
Organizăm codul pe bounded contexts (Workspace, Document, Language, Intelligence, Theming, Terminal, Workbench, Commands, Settings, Extensions), fiecare cu straturile `domain` și `application`. Adaptoarele de infrastructură stau în crate-uri separate. `app` este singurul composition root.

## Alternative
- Un singur crate cu module: mai simplu la început, dar granițele nu sunt impuse de compilator.
- Crate separat pentru fiecare strat al fiecărui context (domain/app/infra): granularitate prea mare, compilare lentă, overhead mare.

## Consecințe
+ Granițele sunt impuse de `Cargo.toml`, iar testele de domeniu rulează rapid și izolat.
+ Compilarea incrementală e eficientă (se recompilează doar crate-urile atinse).
− Mai mult boilerplate (DTO-uri, porturi). Acceptat conștient.
