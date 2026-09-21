# Third-party notices

## lobehub icons (MIT)

Provider brand marks in `ui/src/lib/providerMarks.ts` use path data from the
lobehub icon set, rendered inline by `ProviderLogo.svelte`. No image files.

Upstream: https://github.com/lobehub/lobehub (MIT)

## Fonts (SIL Open Font License 1.1)

The bundle compiles Inter, JetBrains Mono and Instrument Serif (via their
Fontsource packages, as listed in `ui/package.json`) into `ui/dist`.
Each remains under its own OFL terms; see the typefaces' upstream
repositories for license texts.

## Bundled artwork (not licensed for redistribution)

No third-party artwork ships in the bundle. The Evangelion fan wallpapers
previously bundled ("asuka.png", "eva-crosses.jpg") were removed before the
repository went public; existing installs keep their local copies, and old
theme packs referencing them retire on upgrade.
