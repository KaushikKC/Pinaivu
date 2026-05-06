import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./app/**/*.{ts,tsx}", "./components/**/*.{ts,tsx}", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        fraunces: ["Fraunces", "serif"],
        playfair: ["Playfair Display Italic", "serif"],
        mono: ["JetBrains Mono", "monospace"],
        sans: ["Inter Tight", "sans-serif"],
      },
    },
  },
  plugins: [],
};

export default config;
