# ADR-0004: `ropey` acceptat în domeniul Document

**Status:** Acceptat (2026-10-19)

## Context
Domeniul trebuie să fie „pur”, dar structura de date pentru text este însăși esența domeniului Document. O reimplementare proprie ar fi costisitoare și riscantă.

## Decizie
`document` folosește `ropey` intern, ascuns în spatele trait-ului `TextBuffer` și al tipului `TextSnapshot`. `ropey` nu apare în API-ul public al domeniului.

## Consecințe
+ Clonare O(1) pentru snapshot-uri (indexare și parsing pe fundal fără lock-uri).
+ Putem înlocui cu un SumTree propriu dacă vom avea nevoie de CRDT sau de colaborare.
