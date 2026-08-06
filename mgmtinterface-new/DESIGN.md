---
version: alpha
name: ER-C Data Management UI

colors:
  background: "#f1f5fd"
  inverse-background: "#0b1120"
  surface: "#ffffff"
  inverse-surface: "#111827"
  surface-variant: "#f8fafc"
  inverse-surface-variant: "#1e293b"
  outline: "#e2e8f5"
  inverse-outline: "#1e293b"
  on-surface: "#0f172a"
  inverse-on-surface: "#f1f5f9"
  on-surface-variant: "#475569"
  inverse-on-surface-variant: "#cbd5e1"
  on-surface-dim: "#64748b"
  inverse-on-surface-dim: "#94a3b8"
  outline-variant: "#94a3b8"
  inverse-outline-variant: "#64748b"
  primary: "#2563eb"
  inverse-primary: "#3b82f6"
  primary-variant: "#1d4ed8"
  inverse-primary-variant: "#60a5fa"
  on-primary: "#ffffff"
  inverse-on-primary: "#0b1120"
  primary-tint: "rgb(37 99 235 / 0.35)"
  inverse-primary-tint: "rgb(59 130 246 / 0.55)"
  error: "#dc2626"
  inverse-error: "#ef4444"
  error-dim: "#b91c1c"
  inverse-error-dim: "#f87171"
  error-container: "#d827270d"
  inverse-error-container: "#eb3b3b0d"
  error-variant: "#db24244d"
  inverse-error-variant: "#ee42424d"
  tertiary: "#059669"
  inverse-tertiary: "#10b981"
  secondary: "#b45309"
  inverse-secondary: "#f59e0b"
  shadow: "#00000014"
  inverse-shadow: "#00000014"

typography:
  body:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 0.875rem
    fontWeight: 400
  navigation:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 0.875rem
    fontWeight: 500
  label:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 0.875rem
    fontWeight: 600
  button:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 0.875rem
    fontWeight: 600
  heading:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 1.375rem
    fontWeight: 700
  section-heading:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 1.125rem
    fontWeight: 700
  supporting-text:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    fontSize: 0.8125rem
    fontWeight: 400
  login-logo:
    fontSize: 2.25rem
    fontWeight: 400
  mono:
    fontFamily: "JetBrains Mono, ui-monospace, SFMono-Regular, monospace"
    fontSize: 0.875rem
    fontWeight: 400
  mono-medium:
    fontFamily: "JetBrains Mono, ui-monospace, SFMono-Regular, monospace"
    fontSize: 0.875rem
    fontWeight: 500

spacing:
  unit: 4px
  card-padding-x: 40px
  card-padding-y: 44px
  input-padding-x: 12px
  input-padding-y: 10px
  button-padding-x: 20px
  button-padding-y: 10px
  field-label-gap: 6px
  form-field-gap: 16px
  header-title-gap: 14px
  header-subtitle-gap: 4px
  header-bottom-gap: 32px
  button-margin-top: 8px
  app-item-gap-x: 8px
  app-item-gap-y: 10px
  flow-navigation-gap: 6px
  login-back-link-gap: 20px
  app-window-margin-x: 8px
  app-window-margin-y: 10px
  login-error-gap: 16px

rounded:
  lg: 0.5rem
  xl: 0.75rem
  2xl: 1rem
  full: 9999px

components:
  focus-ring:
    width: 3px
  standard-border:
    width: 1px
  app-window-width:
    width: 1024px
  app-window-height:
    height: 768px
  app-shell:
    padding: 27px
  top-bar:
    height: 56px
    padding: 28px
  navigation-progress:
    height: 4px
  top-bar-logo:
    height: 26px
  top-bar-brand-gap:
    size: 6px
  top-bar-brand-divider:
    height: 22px
  top-bar-control-gap:
    size: 2px
  top-bar-control:
    size: 32px
  top-bar-control-divider:
    height: 18px
  top-bar-icon:
    size: 18px
  avatar:
    size: 32px
  project-row:
    padding-y: 12px
  login-card:
    width: 400px
  card-shadow-offset-y:
    size: 1px
  card-shadow-blur:
    size: 4px
  card-shadow-spread:
    size: 0px
  card-shadow-light:
    backgroundColor: "{colors.shadow}"
  card-shadow-dark:
    backgroundColor: "{colors.inverse-shadow}"
  danger-alert-padding-x:
    padding: 14px
  danger-alert-padding-y:
    padding: 12px
  content-card:
    padding-x: 20px
    padding-y: 20px
  error-toast:
    padding-x: 20px
    padding-y: 20px
---

# Design Specification: ER-C Data Management UI

This document serves as the design specification and single source of truth for the visual styles of the ER-C Data Management UI.

## Overview
A clean, spacious, high-contrast user interface designed for data self-service tools. It includes semantic coloring, a robust type system, responsive spacing, and comprehensive dark mode support activated via the `.dark` class.

## Colors
Color variables are mapped directly to theme variables. Hex values must never be hardcoded in component layouts; use semantic variables.

- **Light Mode**: High-contrast blue-grey backgrounds (`#f1f5fd`) with clean white card surfaces (`#ffffff`).
- **Dark Mode**: Deep blue-black backgrounds (`#0b1120`) with dark card surfaces (`#111827`).
- **Accent/Primary**: Bright blue (`#2563eb` light / `#3b82f6` dark) for action components.
- **Feedback States**: `error` red for failures, `tertiary` green for confirmation, and `secondary` amber for warnings.

## Typography
- **Sans font**: `Plus Jakarta Sans` for general reading, labels, and headings.
- **Mono font**: `JetBrains Mono` for displaying resource tags, versions, and code metrics.

## Layout
The interface follows a 4px base rhythm with named spacing tokens for component
padding, form grouping, headers, and the application shell. The native
application defaults to a 1024px × 768px window with 27px content padding.

## Elevation & Depth
Cards use a subtle one-pixel-offset shadow with a four-pixel blur. Borders and
tonal surface differences carry most of the visual hierarchy in both themes.

## Shapes
- **lg (8px / 0.5rem)**: Standard rounding for interactive elements like inputs, text fields, and standard buttons.
- **xl (12px / 0.75rem)**: Moderate rounding used for temporary alerts or toast notification popups.
- **2xl (16px / 1rem)**: Large rounding used for major UI containers, cards, and modal popups.
- **full (9999px)**: Completely round shapes used for user initials or status dots.

## Components

Component geometry is defined in the front matter so that the React and native
implementations share the same dimensions. Single-purpose component entries
use the standard `width`, `height`, `size`, and `padding` sub-tokens supported
by the design.md schema. The application defaults to a 1024px × 768px window
with 27px content padding. Standard outlines and dividers use the 1px
`standard-border` component token.

### Top Bar
The persistent top bar is 56px high with 28px horizontal padding. Its logo,
dividers, action controls, icons, avatar, and spacing use the corresponding
`top-bar` component tokens.

### Button
Supported variants:
- **Primary**: Filled with `--primary` background, `--primary-fg` text. Transitions to `--primary-hover` on hover.
- **Secondary**: Outlined with `outline` and a `surface` background. Transitions to `surface-variant` on hover.
- **Danger**: Filled with `error`. Transitions to `error-dim` on hover.
- **Danger Outline**: Outlined with `error-variant` and a `surface` background. Transitions to `error-container` on hover.

*Focus state: Outlined using the 3px focus-ring width and `primary-tint` color.*

### Card
Containers with `2xl` (16px) corner rounding, an `outline` border, a `surface`
background, and a subtle shadow using the card shadow tokens.

### Input (type=text)
Full-width input field with:
- Standard padding (`px-3 py-2.5`)
- Standard `outline` border
- Corner radius: `lg` (8px)
- Focus states: border turns to `--primary`, focus ring shadow is applied.

### Toast
Bottom-right notification popup using `xl` (12px) corner rounding, left-side
colored border-stripes (`tertiary` or `error` depending on state), and an
elevation shadow.

### Flow navigation
Compact step navigation uses the supporting-text typography and a 6px inline
gap. Completed steps use the primary color, the current step uses the muted
foreground, and future steps use the disabled treatment. The native client
uses this for the `Overview › Project › Session` flow.

### Avatar
Deterministic categorical colored circles (`full`) based on username hashing. Displays initials in bold uppercase. Used palette:
`#2563eb`, `#7c3aed`, `#059669`, `#d97706`, `#dc2626`, `#0891b2`, `#be185d`, `#65a30d`.

### Danger Treatment
Subtle danger panels and danger-outline controls use the `error-container`,
`error-variant`, and alert padding tokens so that feedback treatments remain
consistent across clients.
Disabled native controls use 60% opacity; opacity is documented here because
the alpha DESIGN.md schema does not define a machine-readable opacity token.

## Do's and Don'ts
- Do use semantic color and spacing tokens rather than literal visual values.
- Do preserve WCAG AA contrast for normal text.
- Don't introduce custom front-matter properties outside the published schema.
