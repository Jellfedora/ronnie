# Emojis

- Images: **Twemoji** by Twitter, Inc. and other contributors, licensed under CC-BY 4.0
  (https://creativecommons.org/licenses/by/4.0/). From emoji-datasource-twitter 16.0.0's 64 px sheet,
  cut into one PNG per emoji (64 colours each) packed in `twemoji-64.bin`; `twemoji-64.idx` gives each
  one's sheet cell, offset and length ("x y offset length").
- Emoji list (codes, short names, categories, sheet positions): **emoji-datasource** by Cal Henderson,
  MIT license.
- French names and keywords: **Unicode CLDR** 49 annotations, Unicode License v3.

`emoji.tsv` is generated from these by a script (one line per emoji: category, characters, sheet x, sheet y,
short names, French name, French keywords, skin variations).
