'use client';

import { useEffect, useId, useRef, useState } from 'react';

import styles from '@/components/mermaid.module.css';

/**
 * Track the `dark` class the theme provider puts on the document element.
 * @returns Whether the dark theme is on.
 */
const useIsDark = (): boolean => {
  const [isDark, setIsDark] = useState(false);

  useEffect(() => {
    const root = document.documentElement;
    const read = (): void => {
      setIsDark(root.classList.contains('dark'));
    };
    read();

    const observer = new MutationObserver(read);
    observer.observe(root, { attributeFilter: ['class'] });
    return () => {
      observer.disconnect();
    };
  }, []);

  return isDark;
};

/**
 * A CSS colour as `#rrggbb`, painted through a canvas so the site's oklch tokens reach mermaid as
 * values its colour parser accepts. A colour the browser cannot parse paints the fallback instead.
 * @param context - A 2D context of a one-pixel canvas.
 * @param color - The CSS colour.
 * @param fallback - A hex colour for when `color` does not parse.
 * @returns The colour as `#rrggbb`.
 */
const toHex = (context: CanvasRenderingContext2D, color: string, fallback: string): string => {
  context.clearRect(0, 0, 1, 1);
  context.fillStyle = fallback;
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  const [red = 0, green = 0, blue = 0] = context.getImageData(0, 0, 1, 1).data;
  return `#${[red, green, blue].map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
};

/**
 * Brand `themeVariables` for mermaid's `base` theme, read off the diagram element so the palette
 * lives in CSS next to the rest of the site's tokens and the `.dark` class picks the variant. The
 * tokens are oklch, which mermaid's colour parser (khroma) cannot read, so each is painted to hex
 * first, the way the 3D view resolves its colours.
 * @param element - The diagram's wrapper, which carries the `--mm-*` tokens.
 * @returns The variables.
 */
const themeVariables = (element: HTMLElement): Record<string, string> => {
  const computed = getComputedStyle(element);
  const paint = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  const token = (name: string, fallback: string): string => {
    const value = computed.getPropertyValue(name).trim();
    return paint === null || value === '' ? fallback : toHex(paint, value, fallback);
  };
  const ink = token('--mm-ink', '#1a1a1a');
  const inkMuted = token('--mm-ink-muted', '#6b6b6b');
  const accent = token('--mm-accent', '#2a7a7a');

  return {
    fontFamily: 'inherit',
    fontSize: '14px',
    primaryColor: token('--mm-accent-wash', '#e6f2f2'),
    primaryBorderColor: accent,
    primaryTextColor: ink,
    nodeTextColor: ink,
    textColor: ink,
    lineColor: inkMuted,
    clusterBkg: token('--mm-offset', '#f2f2f2'),
    clusterBorder: token('--mm-line', '#d0d0d0'),
    titleColor: inkMuted,
    // A filter, not a colour, so it reaches mermaid as written.
    dropShadow: computed.getPropertyValue('--mm-shadow').trim() || 'none',
  };
};

/**
 * A mermaid diagram drawn from its source. The source stays a ```mermaid fence in the MDX, so the
 * Markdown endpoints agents read keep the diagram verbatim while readers get it drawn. Mermaid is
 * imported lazily, so pages without a diagram never load it.
 * @param props - The diagram.
 * @param props.chart - The mermaid source.
 * @returns The diagram, or the source as a code block when it cannot be drawn.
 */
export const Mermaid = ({ chart }: { readonly chart: string }): React.JSX.Element => {
  const id = useId().replaceAll(':', '');
  const container = useRef<HTMLDivElement>(null);
  const isDark = useIsDark();
  const [drawn, setDrawn] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const element = container.current;
    if (!element) {
      return undefined;
    }

    const draw = async (): Promise<void> => {
      const { default: mermaid } = await import('mermaid');
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: 'base',
        fontFamily: 'inherit',
        themeVariables: themeVariables(element),
        flowchart: { curve: 'basis', nodeSpacing: 28, rankSpacing: 36, padding: 12 },
      });
      const { svg } = await mermaid.render(`mermaid-${id}`, chart);
      if (!cancelled) {
        // The wrapper has no React children, so the drawn SVG is its content until the next draw.
        element.innerHTML = svg;
        setDrawn(true);
      }
    };

    // A failed import or render falls back to the fence source rather than leaving a blank diagram
    // behind an unhandled rejection.
    void (async () => {
      try {
        await draw();
      } catch {
        if (!cancelled) {
          setFailed(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
    // `isDark` is a dependency only: it reruns the draw so the CSS palette is read again after a
    // theme switch. The values themselves come from the module.
  }, [chart, id, isDark]);

  if (failed) {
    return <pre className={styles.fallback}>{chart}</pre>;
  }

  return <div className={styles.diagram} data-mermaid={drawn ? 'drawn' : 'pending'} ref={container} />;
};
