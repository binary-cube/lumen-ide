# ADR-0005: UI kit propriu

**Status:** Acceptat (2026-10-19)

## Context
Cerințele sunt teme complete, text pe GPU prin pipeline-ul nostru, virtualizare pe milioane de linii, latență minimă și un aspect custom (nu nativ).

## Decizie
Construim `ui-kit`: element tree retained, layout cu `taffy`, focus, input, hit-testing, scroll, liste virtualizate, popup-uri și animații simple. Produce o `Scene` pentru `render-api`.

## Alternative
- **egui:** imediat-mode, ușor de folosit, dar are propriul renderer de text și e greu de stilizat la nivel de IDE.
- **iced:** arhitectură Elm, bun, dar pipeline-ul de text și temele sunt mai puțin flexibile.
- **Slint:** declarativ, licențiere duală, control redus pe randarea editorului.
- **GPUI:** foarte potrivit, dar e cuplat de editorul pentru care a fost făcut și are API instabil. Rămâne o sursă importantă de inspirație.
- **Floem (Lapce):** opțiune viabilă; o putem reevalua dacă UI kit-ul propriu devine prea costisitor.

## Consecințe
+ Control total, fără compromisuri pe partea de editor și teme.
− Cel mai mare risc de timp al proiectului. Îl reducem printr-un scope strict.
