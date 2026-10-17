import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { RootProvider } from 'fumadocs-ui/provider/next';

import { languageMarkup } from '@/lib/language-boot.ts';
import { siteOrigin } from '@/lib/site.ts';

import '@/app/global.css';

export const metadata: Metadata = {
  description: 'GeoSpec, geometry specifications for CAD: state what a model must be, and get pass, fail or refused.',
  icons: { icon: [{ url: '/favicon.svg', type: 'image/svg+xml' }] },
  ...(siteOrigin ? { metadataBase: new URL(siteOrigin) } : {}),
  title: { default: 'GeoSpec', template: '%s — GeoSpec' },
};

const RootLayout = ({ children }: { readonly children: ReactNode }): React.JSX.Element => (
  <html lang="en" suppressHydrationWarning>
    <head>
      {/* Applies the stored language before paint; the theme script Fumadocs adds does the same for colour. */}
      <script dangerouslySetInnerHTML={languageMarkup} />
    </head>
    <body className="flex min-h-screen flex-col">
      <RootProvider>{children}</RootProvider>
    </body>
  </html>
);

export default RootLayout;
