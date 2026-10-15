'use client';

import { useEffect, useRef, useState } from 'react';
import type { LineBasicMaterial, LineDashedMaterial, LineSegments, Vector3 } from 'three';

import styles from '@/components/model-view.module.css';
import type { Box, ModelFocus } from '@/lib/demo/focus.ts';
import { claimedBox } from '@/lib/demo/focus.ts';
import type { DemoMesh } from '@/lib/demo/mesh.ts';
import type { DemoSubject } from '@/lib/demo/subjects.ts';

type Scene = {
  readonly setFocus: (focus: ModelFocus) => void;
  readonly dispose: () => void;
};

type ViewState = 'loading' | 'ready' | 'unavailable';

/** Beyond this many millimetres from the part, a claimed box or ring is not drawn: it would not fit. */
const drawableLimit = 1e5;

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
 * Whether this browser can create a WebGL context, checked without three.js, which logs an error
 * before it throws.
 * @returns `true` when WebGL 2 or WebGL is available.
 */
const hasWebGl = (): boolean => {
  try {
    const canvas = document.createElement('canvas');
    return (canvas.getContext('webgl2') ?? canvas.getContext('webgl')) !== null;
  } catch {
    return false;
  }
};

/**
 * The box the focus draws as the claim's, when it can be drawn.
 * @param focus - The focus.
 * @param reference - The box the claim is read against.
 * @returns The claimed box, or `undefined`.
 */
const drawableClaimedBox = (focus: ModelFocus, reference: Box): Box | undefined => {
  if (focus.kind !== 'bounds') {
    return undefined;
  }
  const box = claimedBox(focus.expected, focus.measured ?? reference);
  const fits = box !== undefined && [...box.min, ...box.max].every((value) => Math.abs(value) < drawableLimit);
  return fits ? box : undefined;
};

/**
 * One sentence saying what the view shows for a focus, for the caption and for readers without WebGL.
 * @param focus - The focus.
 * @param mesh - The display mesh, once loaded.
 * @returns The sentence.
 */
const describeFocus = (focus: ModelFocus, mesh: DemoMesh | undefined): string => {
  switch (focus.kind) {
    case 'bounds': {
      const reference = mesh === undefined ? undefined : (focus.measured ?? meshBox(mesh));
      const claimed = reference === undefined ? undefined : drawableClaimedBox(focus, reference);
      const source = focus.measured === undefined ? 'The model’s bounds' : 'The measured bounds';
      if (claimed === undefined) {
        return `${source} (solid); this claim pins no axis the view can draw.`;
      }
      return `${source} (solid) and the box the claim describes (dashed).`;
    }
    case 'cylinder': {
      const cylinders = mesh?.faces.filter(({ kind }) => kind === 'cylinder').length ?? 0;
      if (cylinders === 0) {
        return 'This claim looks for cylindrical faces; the display mesh has none to mark.';
      }
      return focus.claimedDiameter === undefined
        ? 'The cylindrical faces.'
        : `The cylindrical faces, and the claimed ${focus.claimedDiameter} mm diameter (dashed rings).`;
    }
    case 'plane': {
      return 'The planar faces.';
    }
    case 'body': {
      return 'The whole body: this claim measures all of it.';
    }
    default: {
      return 'Select a claim to see what it checks.';
    }
  }
};

const meshBox = (mesh: DemoMesh): Box => ({
  min: [mesh.bounds.min[0] ?? 0, mesh.bounds.min[1] ?? 0, mesh.bounds.min[2] ?? 0],
  max: [mesh.bounds.max[0] ?? 0, mesh.bounds.max[1] ?? 0, mesh.bounds.max[2] ?? 0],
});

/**
 * Build the three.js scene for a display mesh inside `host` and return its controls.
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
  const reference = meshBox(mesh);
  const [minX, minY, minZ] = reference.min;
  const [maxX, maxY, maxZ] = reference.max;
  const centre = new THREE.Vector3((minX + maxX) / 2, (minY + maxY) / 2, (minZ + maxZ) / 2);
  const radius = Math.max(Math.hypot(maxX - minX, maxY - minY, maxZ - minZ) / 2, 1e-3);

  const camera = new THREE.PerspectiveCamera(24, 1, radius / 50, radius * 200);
  camera.up.set(0, 0, 1);
  camera.position.copy(centre).add(new THREE.Vector3(1.1, -1.6, 1).normalize().multiplyScalar(radius * 5.6));
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(centre);
  controls.enablePan = false;
  controls.minDistance = radius * 2.5;
  controls.maxDistance = radius * 10;
  // The controls aim the camera at their target only on update; the constructor aimed it at the origin.
  controls.update();

  scene.add(new THREE.HemisphereLight(0xff_ff_ff, 0x66_66_66, 2));
  const key = new THREE.DirectionalLight(0xff_ff_ff, 1.6);
  key.position.set(radius * 2, -radius * 3, radius * 4);
  scene.add(key);

  // The body: one material per face kind, so a claim can colour the faces it targets.
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(mesh.positions, 3));
  geometry.setIndex([...mesh.indices]);
  if (mesh.normals.length === mesh.positions.length) {
    geometry.setAttribute('normal', new THREE.Float32BufferAttribute(mesh.normals, 3));
  } else {
    geometry.computeVertexNormals();
  }
  const materialIndex = { other: 0, cylinder: 1, plane: 2 } as const;
  for (const face of mesh.faces) {
    geometry.addGroup(face.first * 3, (face.last - face.first + 1) * 3, materialIndex[face.kind]);
  }
  const faceMaterials = [0, 1, 2].map(
    () => new THREE.MeshStandardMaterial({ metalness: 0.15, roughness: 0.55, side: THREE.DoubleSide }),
  );
  scene.add(new THREE.Mesh(geometry, faceMaterials));

  const edgeMaterial = new THREE.LineBasicMaterial({ transparent: true, opacity: 0.45 });
  const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry, 30), edgeMaterial);
  scene.add(edges);

  // The measured bounds, solid, and the box the claim describes, dashed. Both are rebuilt per focus.
  const boundsMaterial = new THREE.LineBasicMaterial();
  const claimedMaterial = new THREE.LineDashedMaterial({ dashSize: radius * 0.05, gapSize: radius * 0.035 });
  const overlays = new THREE.Group();
  scene.add(overlays);
  const clearOverlays = (): void => {
    for (const child of overlays.children) {
      if (child instanceof THREE.LineSegments || child instanceof THREE.Line) {
        child.geometry.dispose();
      }
    }
    overlays.clear();
  };
  const boxLines = (box: Box, material: LineBasicMaterial | LineDashedMaterial): LineSegments => {
    const size = box.max.map((value, axis) => Math.max(value - (box.min[axis] ?? 0), radius * 1e-3));
    const boxGeometry = new THREE.BoxGeometry(size[0], size[1], size[2]);
    const edges = new THREE.EdgesGeometry(boxGeometry);
    boxGeometry.dispose();
    const lines = new THREE.LineSegments(edges, material);
    lines.position.set((box.min[0] + box.max[0]) / 2, (box.min[1] + box.max[1]) / 2, (box.min[2] + box.max[2]) / 2);
    lines.computeLineDistances();
    return lines;
  };
  const ringPoints = (diameter: number): Vector3[] =>
    Array.from({ length: 97 }, (_, step) => {
      const angle = (step / 96) * Math.PI * 2;
      return new THREE.Vector3(Math.cos(angle) * (diameter / 2), Math.sin(angle) * (diameter / 2), 0);
    });
  const cylinders = mesh.faces.filter((face) => face.kind === 'cylinder' && face.centre !== undefined);

  let focus: ModelFocus = { kind: 'none' };
  const render = (): void => {
    renderer.render(scene, camera);
  };
  const applyFocus = (): void => {
    // A mid grey reads as a machined part on both the light and the dark card.
    const base = '#a3a3a8';
    const ink = themeColor('--color-fd-foreground', '#18181b');
    const accent = focus.kind === 'none' ? ink : themeColor(focus.colorProperty, ink);
    const highlighted = (index: number): boolean =>
      focus.kind === 'body' ||
      (focus.kind === 'cylinder' && index === materialIndex.cylinder) ||
      (focus.kind === 'plane' && index === materialIndex.plane);
    for (const [index, material] of faceMaterials.entries()) {
      material.color.set(highlighted(index) ? accent : base);
    }
    edgeMaterial.color.set(ink);
    boundsMaterial.color.set(accent);
    claimedMaterial.color.set(accent);

    clearOverlays();
    if (focus.kind === 'bounds') {
      overlays.add(boxLines(focus.measured ?? reference, boundsMaterial));
      const claimed = drawableClaimedBox(focus, reference);
      if (claimed !== undefined) {
        overlays.add(boxLines(claimed, claimedMaterial));
      }
    }
    if (focus.kind === 'cylinder' && focus.claimedDiameter !== undefined && focus.claimedDiameter < drawableLimit) {
      for (const face of cylinders) {
        const [cx = 0, cy = 0, cz = 0] = face.centre ?? [];
        const half = (face.length ?? 0) / 2 + radius * 0.005;
        const axis = face.axis ?? 2;
        for (const side of [-1, 1]) {
          const ring = new THREE.Line(
            new THREE.BufferGeometry().setFromPoints(ringPoints(focus.claimedDiameter)),
            claimedMaterial,
          );
          ring.position.set(cx, cy, cz);
          ring.position.setComponent(axis, ring.position.getComponent(axis) + side * half);
          if (axis === 0) {
            ring.rotation.y = Math.PI / 2;
          } else if (axis === 1) {
            ring.rotation.x = Math.PI / 2;
          }
          ring.computeLineDistances();
          overlays.add(ring);
        }
      }
    }
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
  const themeObserver = new MutationObserver(applyFocus);
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style'] });
  controls.addEventListener('change', render);
  resize();
  applyFocus();

  return {
    setFocus: (next) => {
      focus = next;
      applyFocus();
    },
    dispose: () => {
      resizeObserver.disconnect();
      themeObserver.disconnect();
      controls.dispose();
      clearOverlays();
      geometry.dispose();
      edges.geometry.dispose();
      for (const material of [...faceMaterials, edgeMaterial, boundsMaterial, claimedMaterial]) {
        material.dispose();
      }
      renderer.dispose();
      renderer.domElement.remove();
    },
  };
};

/**
 * An interactive 3D view of a demo model, drawn from the display mesh the build tessellates from the
 * same file the engine evaluates. It highlights what the focused claim targets: the faces it reads,
 * the bounds the engine measured, and the box or diameter the claim states.
 * @param props - The model and the highlight.
 * @param props.subject - The model.
 * @param props.focus - What to highlight.
 * @returns The view.
 */
export const ModelView = ({
  subject,
  focus,
}: {
  readonly subject: DemoSubject;
  readonly focus: ModelFocus;
}): React.JSX.Element => {
  const host = useRef<HTMLDivElement>(null);
  const scene = useRef<Scene | undefined>(undefined);
  const latestFocus = useRef(focus);
  const [state, setState] = useState<ViewState>('loading');
  const [mesh, setMesh] = useState<DemoMesh | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    const element = host.current;
    if (element === null) {
      return undefined;
    }
    setState('loading');
    setMesh(undefined);
    const load = async (): Promise<void> => {
      const response = await fetch(subject.meshUrl);
      if (!response.ok) {
        throw new Error(`The display mesh did not load (HTTP ${response.status}).`);
      }
      const loaded = (await response.json()) as DemoMesh;
      if (cancelled) {
        return;
      }
      setMesh(loaded);
      if (!hasWebGl()) {
        setState('unavailable');
        return;
      }
      const created = await createScene(element, loaded);
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
  }, [subject.meshUrl]);

  useEffect(() => {
    latestFocus.current = focus;
    scene.current?.setFocus(focus);
  }, [focus]);

  return (
    <figure className={styles.figure} data-model-view={state}>
      <div aria-label={`3D view: ${subject.description}`} className={styles.viewport} ref={host} role="img">
        {state === 'loading' ? <p className={styles.message}>Loading the model…</p> : undefined}
        {state === 'unavailable' ? (
          <p className={styles.message}>
            The 3D view needs WebGL, which this browser does not provide. The results do not depend on it.
          </p>
        ) : undefined}
      </div>
      <figcaption className={styles.caption}>
        <span>{describeFocus(focus, mesh)}</span>
        {state === 'ready' ? <span> Drag to turn the part.</span> : undefined}
      </figcaption>
    </figure>
  );
};
