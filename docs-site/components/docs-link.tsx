import Link from 'fumadocs-core/link';
import type { ComponentProps } from 'react';

/** Paths the site serves as model files, which the router must not prefetch as pages. */
const modelFile = /\.(?:step|stp|glb|gltf|stl)$/iu;

/**
 * A page link, or a plain download link when the target is a model file. The router prefetches
 * every internal href as a route, and a static file answers that prefetch with a 404 in the console.
 * @param props - The anchor's attributes.
 * @returns The link.
 */
export const DocsLink = (props: ComponentProps<'a'>): React.JSX.Element =>
  modelFile.test(props.href ?? '') ? <a download {...props} /> : <Link {...props} />;
