# Third-party notices

## lobehub icons (MIT)

Provider brand marks in `ui/src/lib/providerMarks.ts` use path data from the
lobehub icon set, rendered inline by `ProviderLogo.svelte`.

Upstream: https://github.com/lobehub/lobehub

## Simple Icons (CC0 1.0)

Connector brand marks in `ui/src/lib/connectorMarks.ts` are path data from
Simple Icons 16.33.0, dedicated to the public domain. The brands remain their
owners' trademarks and show only to name the service a connector reaches.

Upstream: https://github.com/simple-icons/simple-icons

## Octicons (MIT)

Interface icons in `ui/src/lib/icons.ts` are GitHub's Octicons, generated
from the `@primer/octicons` package by `ui/scripts/icons.mjs`.

Upstream: https://github.com/primer/octicons

## Fonts (SIL Open Font License 1.1)

The bundle compiles Inter and JetBrains Mono (via their Fontsource packages,
listed in `ui/package.json`) into `ui/dist`. Each remains under its own OFL
terms; see the typefaces' upstream repositories for license texts.

## adblock-rust (MPL-2.0)

The ad blocker links Brave's `adblock` crate, unmodified, under the Mozilla
Public License 2.0. Source: https://github.com/brave/adblock-rust

## Filter lists

EasyList and EasyPrivacy are not bundled. Parzi downloads them from
https://easylist.to on first use and keeps a copy in `~/.parzi/cache/adblock`.
They are dual-licensed under GPLv3 and CC BY-SA 3.0 by The EasyList authors.
