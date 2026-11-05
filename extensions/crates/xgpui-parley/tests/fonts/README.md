# Font fixture

`InterVariable.ttf` and `Inter-LICENSE` are copied unchanged from the fonts
directory of cosmic-text 0.19.0. Inter is copyright The Inter Project Authors
and distributed under the SIL Open Font License 1.1; see `Inter-LICENSE`.

The fixture verifies real variable-font advances/bounds and fallback against
the repository's IBM Plex Sans fixture. Tests do not discover host fonts.

`NotoSansArabic.ttf`, `NotoSansHebrew.ttf` and the accompanying
`NotoSans-LICENSE` are copied unchanged from cosmic-text 0.19.0's fonts directory.
They are SIL Open Font License 1.1 fixtures for deterministic mixed-script
paragraph tests, without host font discovery.

`RobotoFlex.ttf` is copied unchanged from Google Fonts `ofl/robotoflex` at
revision `8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5`; accompanying license: `RobotoFlex-OFL.txt` (SIL OFL 1.1).
SHA-256: `9b523f7d82593df0107173849ebb8c817471a1df4b4fb2c3cbf40cfd810c8281`.
It supplies real negative `slnt`, `wdth`, `wght`, and `opsz` fixtures.
