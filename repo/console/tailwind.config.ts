import type { Config } from "tailwindcss";
import tailwindcssAnimate from "tailwindcss-animate";

const config = {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    container: { center: true, padding: "2rem", screens: { "2xl": "1400px" } },
    extend: {
      spacing: { "0.25": "1px", "0.75": "3px", "1.25": "5px" },
      fontFamily: {
        sans: ["Inter", "SF Pro Text", "Segoe UI", "ui-sans-serif", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: { DEFAULT: "hsl(var(--primary))", foreground: "hsl(var(--primary-foreground))" },
        secondary: { DEFAULT: "hsl(var(--secondary))", foreground: "hsl(var(--secondary-foreground))" },
        muted: { DEFAULT: "hsl(var(--muted))", foreground: "hsl(var(--muted-foreground))" },
        accent: { DEFAULT: "hsl(var(--accent))", foreground: "hsl(var(--accent-foreground))" },
        destructive: { DEFAULT: "hsl(var(--destructive))", foreground: "hsl(var(--destructive-foreground))" },
        card: { DEFAULT: "hsl(var(--card))", foreground: "hsl(var(--card-foreground))" },
        ok: { DEFAULT: "hsl(var(--ok))", foreground: "hsl(var(--ok-foreground))" },
        warn: { DEFAULT: "hsl(var(--warn))", foreground: "hsl(var(--warn-foreground))" },
        info: { DEFAULT: "hsl(var(--info))", foreground: "hsl(var(--info-foreground))" },
        // Ledger semantic pair — credit (money in) / debit (money out).
        credit: "hsl(var(--credit))",
        debit: "hsl(var(--debit))",
        surface: {
          app: "hsl(var(--surface-app))",
          sidebar: "hsl(var(--surface-sidebar))",
          panel: "hsl(var(--surface-panel))",
          header: "hsl(var(--surface-header))",
          toolbar: "hsl(var(--surface-toolbar))",
          input: "hsl(var(--surface-input))",
          hover: "hsl(var(--surface-hover))",
          active: "hsl(var(--surface-active))",
        },
        outline: {
          subtle: "hsl(var(--outline-subtle))",
          strong: "hsl(var(--outline-strong))",
        },
        icon: {
          muted: "hsl(var(--icon-muted))",
          active: "hsl(var(--icon-active))",
          tile: "hsl(var(--icon-tile))",
          "tile-foreground": "hsl(var(--icon-tile-foreground))",
        },
        // swiftpipe product planes — one accent per functional area.
        plane: {
          pipeline: "hsl(var(--plane-pipeline))",
          parser: "hsl(var(--plane-parser))",
          books: "hsl(var(--plane-books))",
          control: "hsl(var(--plane-control))",
          regulatory: "hsl(var(--plane-regulatory))",
          system: "hsl(var(--plane-system))",
        },
      },
      borderRadius: { lg: "var(--radius)", md: "calc(var(--radius) - 2px)", sm: "calc(var(--radius) - 4px)" },
      keyframes: {
        shimmer: { "100%": { transform: "translateX(100%)" } },
        "flash-row": {
          "0%": { background: "hsl(var(--primary) / 0.28)" },
          "100%": { background: "transparent" },
        },
      },
      animation: { "flash-row": "flash-row 1.1s ease-out" },
    },
  },
  plugins: [tailwindcssAnimate],
} satisfies Config;

export default config;
