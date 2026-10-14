'use client';

import { useEffect, useRef, useState } from 'react';

import styles from '@/components/model-view.module.css';
import type { DemoMesh } from '@/lib/demo/mesh.ts';
import { demoMeshUrl } from '@/lib/demo/mesh.ts';

/**
 * What the view highlights: the feature one claim targets, or nothing. `colorProperty` names the CSS
 * custom property the highlight takes its colour from, so it follows the theme.
 */
export type ModelFocus =
  | { readonly kind: 'none' }
  | { readonly kind: 'bounds'; readonly colorProperty: string }
  | { readonly kind: 'cylinder'; readonly colorProperty: string; readonly claimedDiameter: number }
  | { readonly kind: 'body'; readonly colorProperty: string };

type Scene = {
  readonly setFocus: (focus: ModelFocus) => void;
  readonly dispose: () => void;
};

type ViewState = 'loading' | 'ready' | 'unavailable';

/**
 * Resolve any CSS colour, `oklch()` included, to an sRGB hex string by painting one pixel.
 * @param context - A 1 × 1 canvas context to paint with.
 * @param color - The CSS colour.
 * @returns The colour as `#rrggbb`.
 */
const toHex = (context: CanvasRenderingContext2D, color: string): string => {
  context.clearRect(0, 0, 1, 1);
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  const [red = 0, green = 0, blue = 0] = context.getImageData(0, 0, 1, 1).data;
  return `#${[red, green, blue].map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
};

/**
 * Build the three.js scene for the demo mesh inside `host` and return its controls.
 * @param host - The element the canvas fills.
 * @param mesh - The display mesh.
 * @returns The scene's focus setter and disposer.
 */
const createScene = async (host: HTMLElement, mesh: DemoMesh): Promise<Scene> => {
  const THREE = await import('three');
  const { OrbitControls } = await import('three/addons/controls/OrbitControls.js');

  const paint = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  const cssColor = (value: string): string => (paint === null ? '#888888' : toHex(paint, value));
  const themeColor = (name: string, fallback: string): string => {
    const value = getComputedStyle(host).getPropertyValue(name).trim();
    return cssColor(value === '' ? fallback : value);
  };

  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio, 2));
  renderer.domElement.className = styles.canvas ?? '';
  host.append(renderer.domElement);

  const scene = new THREE.Scene();
  const [minX = 0, minY = 0, minZ = 0] = mesh.bounds.min;
  const [maxX = 0, maxY = 0, maxZ = 0] = mesh.bounds.max;
  const centre = new THREE.Vector3((minX + maxX) / 2, (minY + maxY) / 2, (minZ + maxZ) / 2);
  const radius = Math.hypot(maxX - minX, maxY - minY, maxZ - minZ) / 2;

  const camera = new THREE.PerspectiveCamera(24, 1, radius / 50, radius * 50);
  camera.up.set(0, 0, 1);
  camera.position.copy(centre).add(new THREE.Vector3(1.1, -1.6, 1).normalize().multiplyScalar(radius * 5.6));
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(centre);
  controls.enablePan = false;
  controls.minDistance = radius * 2.5;
  controls.maxDistance = radius * 10;

  scene.add(new THREE.HemisphereLight(0xff_ff_ff, 0x66_66_66, 2));
  const key = new THREE.DirectionalLight(0xff_ff_ff, 1.6);
  key.position.set(radius * 2, -radius * 3, radius * 4);
  scene.add(key);

  // The body: one material for every face, a second for the faces a claim targets.
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(mesh.positions, 3));
  if (mesh.normals.length === mesh.positions.length) {
    geometry.setAttribute('normal', new THREE.Float32BufferAttribute(mesh.normals, 3));
  } else {
    geometry.computeVertexNormals();
  }
  geometry.setIndex([...mesh.indices]);
  for (const face of mesh.faces) {
    geometry.addGroup(face.first * 3, (face.last - face.first + 1) * 3, face.kind === 'cylinder' ? 1 : 0);
  }
  const baseMaterial = new THREE.MeshStandardMaterial({ metalness: 0.15, roughness: 0.55 });
  const featureMaterial = new THREE.MeshStandardMaterial({ metalness: 0.15, roughness: 0.55 });
  const body = new THREE.Mesh(geometry, [baseMaterial, featureMaterial]);
  scene.add(body);

  const edgeMaterial = new THREE.LineBasicMaterial({ transparent: true, opacity: 0.45 });
  const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry, 30), edgeMaterial);
  scene.add(edges);

  // The bounding box the size claim measures, drawn just outside the body.
  const boundsMaterial = new THREE.LineBasicMaterial();
  const box = new THREE.Box3(new THREE.Vector3(minX, minY, minZ), new THREE.Vector3(maxX, maxY, maxZ));
  const boundsHelper = new THREE.Box3Helper(box.expandByScalar(radius * 0.01));
  boundsHelper.material = boundsMaterial;
  scene.add(boundsHelper);

  // The diameter the bore claim declares, as a dashed ring at each end of the cylinder.
  const cylinder = mesh.faces.find((face) => face.kind === 'cylinder');
  const claimedMaterial = new THREE.LineDashedMaterial({ dashSize: radius * 0.04, gapSize: radius * 0.03 });
  const claimedRings = new THREE.Group();
  scene.add(claimedRings);
  const setClaimedDiameter = (diameter: number): void => {
    for (const child of claimedRings.children) {
      if (child instanceof THREE.Line) {
        child.geometry.dispose();
      }
    }
    claimedRings.clear();
    const points = Array.from({ length: 97 }, (_, step) => {
      const angle = (step / 96) * Math.PI * 2;
      return new THREE.Vector3(Math.cos(angle) * (diameter / 2), Math.sin(angle) * (diameter / 2), 0);
    });
    for (const z of [minZ, maxZ]) {
      const ring = new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), claimedMaterial);
      ring.position.set(centre.x, centre.y, z + (z === maxZ ? 0.1 : -0.1));
      ring.computeLineDistances();
      claimedRings.add(ring);
    }
  };

  let focus: ModelFocus = { kind: 'none' };
  const render = (): void => {
    renderer.render(scene, camera);
  };
  const applyColors = (): void => {
    // A mid grey reads as a machined part on both the light and the dark card.
    const base = '#a3a3a8';
    const ink = themeColor('--color-fd-foreground', '#18181b');
    const accent = focus.kind === 'none' ? ink : themeColor(focus.colorProperty, ink);
    baseMaterial.color.set(focus.kind === 'body' ? accent : base);
    featureMaterial.color.set(focus.kind === 'cylinder' || focus.kind === 'body' ? accent : base);
    edgeMaterial.color.set(ink);
    boundsMaterial.color.set(accent);
    claimedMaterial.color.set(accent);
    boundsHelper.visible = focus.kind === 'bounds';
    claimedRings.visible = focus.kind === 'cylinder' && cylinder !== undefined;
    render();
  };

  const resize = (): void => {
    const { clientWidth, clientHeight } = host;
    if (clientWidth === 0 || clientHeight === 0) {
      return;
    }
    renderer.setSize(clientWidth, clientHeight, false);
    camera.aspect = clientWidth / clientHeight;
    camera.updateProjectionMatrix();
    render();
  };
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(host);
  // The theme toggles a class on the root element; re-read the colours when it does.
  const themeObserver = new MutationObserver(applyColors);
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style'] });
  controls.addEventListener('change', render);
  resize();
  applyColors();

  return {
    setFocus: (next) => {
      focus = next;
      if (next.kind === 'cylinder') {
        setClaimedDiameter(next.claimedDiameter);
      }
      applyColors();
    },
    dispose: () => {
      resizeObserver.disconnect();
      themeObserver.disconnect();
      controls.dispose();
      setClaimedDiameter(0);
      geometry.dispose();
      edges.geometry.dispose();
      boundsHelper.geometry.dispose();
      for (const material of [baseMaterial, featureMaterial, edgeMaterial, boundsMaterial, claimedMaterial]) {
        material.dispose();
      }
      renderer.dispose();
      renderer.domElement.remove();
    },
  };
};

/**
 * An interactive 3D view of the demo model, drawn from the display mesh the build tessellates from
 * the same STEP file the engine evaluates. It highlights the feature the focused claim targets.
 * @param props - The highlight.
 * @param props.focus - What to highlight.
 * @returns The view.
 */
export const ModelView = ({ focus }: { readonly focus: ModelFocus }): React.JSX.Element => {
  const host = useRef<HTMLDivElement>(null);
  const scene = useRef<Scene | undefined>(undefined);
  const latestFocus = useRef(focus);
  const [state, setState] = useState<ViewState>('loading');

  useEffect(() => {
    let cancelled = false;
    const element = host.current;
    if (element === null) {
      return undefined;
    }
    const load = async (): Promise<void> => {
      const response = await fetch(demoMeshUrl);
      if (!response.ok) {
        throw new Error(`The demo mesh did not load (HTTP ${response.status}).`);
      }
      const mesh = (await response.json()) as DemoMesh;
      if (cancelled) {
        return;
      }
      const created = await createScene(element, mesh);
      if (cancelled) {
        created.dispose();
        return;
      }
      scene.current = created;
      created.setFocus(latestFocus.current);
      setState('ready');
    };
    void (async () => {
      try {
        await load();
      } catch {
        // No WebGL, or the mesh is missing: the results still stand on their own.
        if (!cancelled) {
          setState('unavailable');
        }
      }
    })();
    return () => {
      cancelled = true;
      scene.current?.dispose();
      scene.current = undefined;
    };
  }, []);

  useEffect(() => {
    latestFocus.current = focus;
    scene.current?.setFocus(focus);
  }, [focus]);

  return (
    <figure className={styles.figure} data-model-view={state}>
      <div
        aria-label="3D view of the demo plate: a 40 by 40 by 20 mm block with a 10 mm hole through its centre"
        className={styles.viewport}
        ref={host}
        role="img"
      >
        {state === 'loading' ? <p className={styles.message}>Loading the model…</p> : undefined}
        {state === 'unavailable' ? (
          <p className={styles.message}>The 3D view needs WebGL, which this browser does not provide.</p>
        ) : undefined}
      </div>
      <figcaption className={styles.caption}>Drag to turn the part. Select a claim to see what it checks.</figcaption>
    </figure>
  );
};
