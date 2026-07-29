---
version: alpha
name: ER-C Data Management UI

colors:
  light:
    bg: "#f1f5fd"
    surface: "#ffffff"
    surface-muted: "#f8fafc"
    border: "#e2e8f5"
    fg: "#0f172a"
    fg-muted: "#475569"
    fg-subtle: "#64748b"
    fg-faint: "#94a3b8"
    primary: "#2563eb"
    primary-hover: "#1d4ed8"
    primary-fg: "#ffffff"
    danger: "#dc2626"
    danger-hover: "#b91c1c"
    success: "#059669"
    warning: "#b45309"
    ring: "rgb(37 99 235 / 0.35)"
  dark:
    bg: "#0b1120"
    surface: "#111827"
    surface-muted: "#1e293b"
    border: "#1e293b"
    fg: "#f1f5f9"
    fg-muted: "#cbd5e1"
    fg-subtle: "#94a3b8"
    fg-faint: "#64748b"
    primary: "#3b82f6"
    primary-hover: "#60a5fa"
    primary-fg: "#0b1120"
    danger: "#ef4444"
    danger-hover: "#f87171"
    success: "#10b981"
    warning: "#f59e0b"
    ring: "rgb(59 130 246 / 0.55)"

typography:
  sans:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    weights: [400, 500, 600, 700]
  body:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans Regular"
    fontSize: 0.875rem
    fontWeight: 400
  navigation:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans Medium"
    fontSize: 0.875rem
    fontWeight: 500
  label:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans SemiBold"
    fontSize: 0.875rem
    fontWeight: 600
  button:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans SemiBold"
    fontSize: 0.875rem
    fontWeight: 600
  heading:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans Bold"
    fontSize: 1.375rem
    fontWeight: 700
  login-title:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans Bold"
    fontSize: 1.125rem
    fontWeight: 700
  login-subtitle:
    fontFamily: "Plus Jakarta Sans, system-ui, -apple-system, sans-serif"
    nativeFontFamily: "Plus Jakarta Sans Regular"
    fontSize: 0.8125rem
    fontWeight: 400
  login-logo:
    fontSize: 2.25rem
    fontWeight: 400
  mono:
    fontFamily: "JetBrains Mono, ui-monospace, SFMono-Regular, monospace"
    nativeFontFamily: "JetBrains Mono Regular"
    fontSize: 0.875rem
    fontWeight: 400
    weights: [400, 500]
  mono-medium:
    fontFamily: "JetBrains Mono, ui-monospace, SFMono-Regular, monospace"
    nativeFontFamily: "JetBrains Mono Medium"
    fontSize: 0.875rem
    fontWeight: 500

spacing:
  unit: 4px
  scale: [0, 1, 1.5, 2, 2.5, 3, 3.5, 4, 5, 6, 7, 8, 10, 11, 12, 14, 16]

shapes:
  radius-lg: 0.5rem
  radius-xl: 0.75rem
  radius-2xl: 1rem
  radius-full: 9999px
  focus-ring-width: 3px

components:
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
  login-card:
    width: 400px
  card-shadow-offset-y:
    size: 1px
  card-shadow-blur:
    size: 4px
  card-shadow-spread:
    size: 0px
  card-shadow-alpha:
    size: 20
  interaction-disabled-alpha:
    size: 0.6
  danger-subtle-fill-alpha:
    size: 0.05
  danger-border-alpha:
    size: 0.30
  danger-alert-padding-x:
    padding: 14px
  danger-alert-padding-y:
    padding: 12px
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
- **Feedback States**: Red (`--danger`) for errors, Green (`--success`) for confirmation, Amber (`--warning`) for warnings.

## Typography
- **Sans font**: `Plus Jakarta Sans` for general reading, labels, and headings.
- **Mono font**: `JetBrains Mono` for displaying resource tags, versions, and code metrics.

## Shapes & Corner Radii
- **radius-lg (8px / 0.5rem)**: Standard rounding for interactive elements like inputs, text fields, and standard buttons.
- **radius-xl (12px / 0.75rem)**: Moderate rounding used for temporary alerts or toast notification popups.
- **radius-2xl (16px / 1rem)**: Large rounding used for major UI containers, cards, and modal popups.
- **radius-full (9999px)**: Completely round shapes used for user initials or status dots.

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
- **Secondary**: Outlined with `--border` and `--surface` background. Transitions to `--surface-muted` on hover.
- **Danger**: Filled with `--danger` background. Transitions to `--danger-hover` on hover.
- **Danger Outline**: Outlined with `--danger/30` border, `--surface` background. Transitions to `--danger/5` on hover.

*Focus state: Outlined using `focus:shadow-[0_0_0_3px_var(--ring)]`.*

### Card
Containers with `radius-2xl` (16px) corner rounding, a default border (`--border`), a white/dark surface background (`--surface`), and a subtle shadow using the `card` component tokens.

### Input (type=text)
Full-width input field with:
- Standard padding (`px-3 py-2.5`)
- Standard border (`border border-default`)
- Corner radius: `radius-lg` (8px)
- Focus states: border turns to `--primary`, focus ring shadow is applied.

### Toast
Bottom-right notification popup using `radius-xl` (12px) corner rounding, left-side colored border-stripes (`--success` or `--danger` depending on state), and elevation shadow (`shadow-lg`).

### Avatar
Deterministic categorical colored circles (`radius-full`) based on username hashing. Displays initials in bold uppercase. Used palette:
`#2563eb`, `#7c3aed`, `#059669`, `#d97706`, `#dc2626`, `#0891b2`, `#be185d`, `#65a30d`.

### Danger Treatment
Subtle danger panels and danger-outline controls use the component alpha and
padding tokens so that feedback treatments remain consistent across clients.
