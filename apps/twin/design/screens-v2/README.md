# twin — screens, v2

The eight twin screens redrawn as a consistent design: Home (`Main`),
World, Chat, Call, Teach, Mind, Worlds & twins (`Friends`) and Key, plus
three shared parts — the pixel twin (`Avatar`), the morse strip (`Morse`)
and the tab bar (`Nav`).

- **Look:** a near-black night ground, mint for the twin and for actions,
  amber morse code, Silkscreen pixel type for labels and Inter for reading.
- **World and Home's room** are pixel art on one 3 px grid (shaded trees,
  lamp, moon, dithered sky and glow, cobblestones), so the twin and its
  world share one bit-style.
- **The twin** is the original 10×16 sprite with the morse collar and badge;
  its colours (hair, skin, shirt, trousers, collar) are settings on the part.
- The tabs link between screens, so the set can be clicked through.

## Format

These are Design-canvas sources (`.dc.html` plus `canvas.json`), not
standalone pages: they load the canvas runtime (`support.js`), so a browser
opened on the file alone shows nothing. View and edit them on the Design
canvas they were made on, or treat them as the reference the real vieww
screens get rendered against — the earlier SVG set beside this folder
(`../screens/`, `../gallery.html`) stays as the first version.
