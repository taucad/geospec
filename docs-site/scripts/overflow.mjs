// Phone-width overflow probe for the static export, shared by the route sweep and the live demo
// e2e. The docs layout clips horizontal overflow, so a page that is too wide does not scroll: its
// right edge is cut off and `scrollWidth` still reads the viewport width. The probe walks the boxes
// instead and names each one that reaches past the viewport outside a horizontal scroll container.

/**
 * Name the boxes that reach past the viewport outside any horizontal scroll container. A box cut
 * by a clipping container narrower than the viewport, such as the editor's highlight layer, is
 * that component's to show and is not counted; a clip spanning the viewport is the page's own and
 * hides nothing on purpose. Runs in the page: pass it to `page.evaluate`.
 * @returns {string[]} One line per outermost overflowing box, empty when the page fits.
 */
export const findOverflow = () => {
  const { document, getComputedStyle } = globalThis;
  const width = document.documentElement.clientWidth;
  const isHidden = (element) => {
    for (let node = element; node !== null; node = node.parentElement) {
      const style = getComputedStyle(node);
      if (style.display === 'none' || style.visibility === 'hidden') {
        return true;
      }
    }
    return false;
  };
  const isContained = (element) => {
    for (let node = element.parentElement; node !== null; node = node.parentElement) {
      const { overflowX } = getComputedStyle(node);
      if (overflowX === 'auto' || overflowX === 'scroll') {
        return true;
      }
      if (overflowX !== 'visible') {
        const box = node.getBoundingClientRect();
        return box.left > 0.5 || box.right < width - 0.5;
      }
    }
    return false;
  };
  const nameOf = (element) => {
    const marker = [...element.attributes].find(({ name }) => name.startsWith('data-') && name !== 'data-state');
    const module = [...element.classList].find((name) => name.includes('-module__'));
    const label = module === undefined ? element.classList[0] : module.replace(/^(.+)-module__\w+__(.+)$/u, '$1.$2');
    return [
      element.tagName.toLowerCase(),
      element.id === '' ? '' : `#${element.id}`,
      label === undefined ? '' : `.${label}`,
      marker === undefined ? '' : `[${marker.name}]`,
    ].join('');
  };
  const pathOf = (element) => {
    const names = [];
    for (let node = element; node !== null && node !== document.body && names.length < 4; node = node.parentElement) {
      names.unshift(nameOf(node));
    }
    return names.join(' > ');
  };
  const overflowing = new Set();
  const found = [];
  for (const element of document.body.querySelectorAll('*')) {
    const box = element.getBoundingClientRect();
    const svg = element.closest('svg');
    if (box.width < 2 || box.height < 2 || (box.left >= -1 && box.right <= width + 1) || (svg !== null && svg !== element)) {
      continue;
    }
    if (getComputedStyle(element).position === 'fixed' || isHidden(element) || isContained(element)) {
      continue;
    }
    overflowing.add(element);
    if (!overflowing.has(element.parentElement)) {
      found.push(`${pathOf(element)} spans ${Math.round(box.left)} to ${Math.round(box.right)} px in a ${width} px viewport`);
    }
  }
  return found;
};
