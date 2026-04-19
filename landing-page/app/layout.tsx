import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Pinaivu AI — Cryptographic-Native Inference Protocol",
  description:
    "Pinaivu AI grounds every guarantee in Ed25519 signatures and SHA-256 Merkle proofs — not a coordinator, not a token.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" data-theme="dark">
      <head>
        <script
          dangerouslySetInnerHTML={{
            __html: `(function(){try{var s=localStorage.getItem('pinaivu-theme')||localStorage.getItem('peerai-theme');if(s==='light'||s==='dark')document.documentElement.setAttribute('data-theme',s);}catch(e){}})();`,
          }}
        />
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link
          rel="preconnect"
          href="https://fonts.gstatic.com"
          crossOrigin="anonymous"
        />
        <link
          href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,300;9..144,400;9..144,500;9..144,600;9..144,700;9..144,800&family=JetBrains+Mono:wght@300;400;500;600;700&family=Inter+Tight:wght@300;400;500;600;700;800&display=swap"
          rel="stylesheet"
        />
      </head>
      <body>{children}</body>
    </html>
  );
}
