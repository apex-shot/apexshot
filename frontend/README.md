# ApexShot Landing Page

A beautiful, modern landing page for ApexShot — a premium screen capture tool for Linux.

## Features

- ✨ **Mesh Gradient Background** — Animated gradient blobs with blur effects
- 🎯 **Spotlight Cards** — Mouse-following spotlight effect on cards
- 📜 **Marquee** — Infinite scrolling animations
- 🎨 **Noise Texture** — Subtle film grain effect
- 📱 **Fully Responsive** — Works on all devices

## Tech Stack

- Next.js 16 (App Router)
- React 19
- TypeScript
- Tailwind CSS 4
- Framer Motion

## Project Structure

```
frontend/
├── app/
│   ├── page.tsx              # Main landing page
│   ├── layout.tsx            # Root layout
│   ├── globals.css           # Global styles
│   ├── sections/             # Page sections
│   │   ├── hero.tsx
│   │   ├── features.tsx
│   │   ├── trust.tsx
│   │   ├── pricing.tsx
│   │   ├── comparison.tsx
│   │   ├── cta.tsx
│   │   ├── faq.tsx
│   │   └── footer.tsx
│   └── components/           # (auto-generated)
├── components/
│   ├── ui/                   # Reusable UI components
│   │   ├── mesh-gradient.tsx
│   │   ├── spotlight.tsx
│   │   ├── marquee.tsx
│   │   ├── noise.tsx
│   │   └── faq.tsx
│   └── navigation.tsx        # Header navigation
├── lib/
│   └── utils.ts              # Utility functions (cn)
├── public/                   # Static assets
└── next.config.ts            # Next.js configuration
```

## Getting Started

```bash
# Install dependencies
pnpm install

# Run development server
pnpm dev

# Build for production
pnpm build
```

## UI Components

### MeshGradient
Animated gradient background with floating blobs.

```tsx
<MeshGradient intensity="medium" />  // low | medium | high
```

### Spotlight
Mouse-following spotlight effect for cards.

```tsx
<Spotlight>
  <SpotLightItem>
    <CardContent />
  </SpotLightItem>
</Spotlight>
```

### Marquee
Infinite scrolling container.

```tsx
<Marquee pauseOnHover>
  <Item />
  <Item />
</Marquee>
```

### Noise
Subtle noise texture overlay.

```tsx
<Noise opacity={0.02} />
```

## License

MIT License
