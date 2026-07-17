# Cloud and filigree assets

The rose mockups use several related pale-gold line ornaments rather than one stretched image. All SVG files are transparent, default to the `ornamentGold` token (`#e5bc96`), and can be recolored when inlined or used as CSS masks.

## Page mapping

| Page | Assets |
| --- | --- |
| Dashboard | `cloud-filigree-corner.svg`, `cloud-segment-bracket.svg`, `cloud-botanical-sprig.svg` |
| Quota | `cloud-panel-9slice.svg` |
| Agents | `cloud-filigree-corner.svg`, `cloud-card-9slice.svg`, `cloud-avatar-wreath.svg` |
| Request logs | `cloud-filigree-corner.svg`, `cloud-rail-junction.svg` |
| Proxy logs | `cloud-filigree-corner.svg`, `cloud-rail-junction.svg` |
| Settings | `cloud-filigree-corner.svg` |
| About | `cloud-panel-9slice.svg`, `cloud-botanical-sprig.svg`, `cloud-avatar-wreath.svg` |

Providers, 2FA and the floating window use normal borders and floral PNG assets; they do not need the cloud-line set.

## Single page corner

```css
.rose-cloud-corner {
  position: absolute;
  width: 240px;
  height: 128px;
  pointer-events: none;
  background: url("/rose/cloud-filigree-corner.svg") center / contain no-repeat;
}

.rose-cloud-corner--tr { transform: scaleX(-1); }
.rose-cloud-corner--bl { transform: scaleY(-1); }
.rose-cloud-corner--br { transform: scale(-1); }
```

Use normal CSS borders for the long straight edges. This keeps the curved ornament from stretching.

## Asymmetric panel frame

```css
.rose-ornate-panel::before {
  content: "";
  position: absolute;
  inset: 0;
  border: 64px solid transparent;
  border-image: url("/rose/cloud-panel-9slice.svg") 64 fill / 64px / 0 stretch;
  pointer-events: none;
}
```

The bottom-right corner is intentionally larger, matching the About and Quota references.

## Small card hooks

```css
.rose-hook-card::before {
  content: "";
  position: absolute;
  inset: 0;
  border: 20px solid transparent;
  border-image: url("/rose/cloud-card-9slice.svg") 20 fill / 20px / 0 stretch;
  pointer-events: none;
}
```

## Log sidebar junction

Place `cloud-rail-junction.svg` at the sidebar/content boundary. Rotate it `180deg` for the bottom junction.

```css
.rose-rail-knot {
  position: absolute;
  left: var(--sidebar-width);
  width: 160px;
  height: 72px;
  transform: translateX(-50%);
  background: url("/rose/cloud-rail-junction.svg") center / contain no-repeat;
  pointer-events: none;
}

.rose-rail-knot--bottom {
  transform: translateX(-50%) rotate(180deg);
}
```

## Character ornaments

Use `cloud-botanical-sprig.svg` on either side of a hero portrait and mirror one copy. Use `cloud-avatar-wreath.svg` behind circular avatars; keep both behind the image with `pointer-events: none`.
