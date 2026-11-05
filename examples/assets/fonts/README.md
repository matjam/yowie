# Example fonts

Fonts used by the `text` example. Each is under the SIL Open Font License 1.1;
the licence text sits beside the font. They are not part of the library.

| File | Source | Licence file |
|---|---|---|
| `NotoSansArabic-Regular.ttf` | Noto Sans Arabic, `notofonts/arabic`, hinted static build, unchanged | `NotoSansArabic-OFL.txt` |
| `NotoSansCJKsc-subset.otf` | Noto Sans CJK SC Regular, `notofonts/noto-cjk`, subset to the characters the example draws with `pyftsubset` | `NotoSansCJK-OFL.txt` |
| `NotoColorEmoji-subset.ttf` | Noto Color Emoji v2.047 (CBDT bitmaps), `googlefonts/noto-emoji`, subset to seven emoji with `pyftsubset` | `NotoColorEmoji-OFL.txt` |

To rebuild a subset, run `pyftsubset <font> --text="<characters>"
--output-file=<name> --layout-features='*'`; for the emoji use
`--unicodes=U+1F600,U+1F680,U+2764,U+1F389,U+1F431,U+1F525,U+2B50`.
