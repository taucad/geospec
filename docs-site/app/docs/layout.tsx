import type { ReactNode } from 'react';
import { DocsLayout } from 'fumadocs-ui/layouts/docs';

import { baseOptions, engineVersion } from '@/app/layout.config.tsx';
import { TauAttributionFooter } from '@/components/tau-attribution-footer.tsx';
import { source } from '@/lib/source.ts';

const DocsShellLayout = ({ children }: { readonly children: ReactNode }): React.JSX.Element => (
  <DocsLayout
    {...baseOptions}
    nav={{
      ...baseOptions.nav,
      // Fumadocs wraps this slot in a flex box spanning the row's full height. Without centring,
      // text this small sits at the top of that box and rides above the brand mark beside it.
      children: (
        <span
          aria-label={`geospec-engine ${engineVersion}`}
          className="ms-1.5 self-center text-fd-muted-foreground text-xs"
        >
          engine {engineVersion}
        </span>
      ),
    }}
    sidebar={{ footer: <TauAttributionFooter /> }}
    tree={source.pageTree}
  >
    {children}
  </DocsLayout>
);

export default DocsShellLayout;
