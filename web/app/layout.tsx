import type { Metadata, Viewport } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title:       'Pinaivu — Decentralised AI',
  description: 'Private, censorship-resistant AI inference on a decentralised GPU network',
  icons: {
    icon: '/favicon.svg',
  },
};

export const viewport: Viewport = {
  themeColor: '#0f0f10',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark h-full">
      <body className="h-full overflow-hidden bg-surface text-white">
        {children}
      </body>
    </html>
  );
}
