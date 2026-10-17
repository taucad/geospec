// Phone-width overflow probes for the static export, shared by the route sweep and the live demo
// e2e. The docs layout clips horizontal overflow, so a page that is too wide does not scroll: its
// right edge is cut off and `scrollWidth` still reads the viewport width. `findOverflow` walks the
// boxes instead and names each one that reaches past the viewport outside a horizontal scroll
// container; `findClipped` names text that a narrower clipping box, such as a table, cuts off.

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
    if (
      box.width < 2 ||
      box.height < 2 ||
      (box.left >= -1 && box.right <= width + 1) ||
      (svg !== null && svg !== element)
    ) {
      continue;
    }
    if (getComputedStyle(element).position === 'fixed' || isHidden(element) || isContained(element)) {
      continue;
    }
    overflowing.add(element);
    if (!overflowing.has(element.parentElement)) {
      found.push(
        `${pathOf(element)} spans ${Math.round(box.left)} to ${Math.round(box.right)} px in a ${width} px viewport`,
      );
    }
  }
  return found;
};

/**
 * Name the text that a clipping box narrower than the viewport cuts off: a word cut by a table
 * that hides its own overflow does not reach past the viewport, so `findOverflow` passes it. Text
 * inside a horizontal scroll container can be scrolled to, text the box ends with an ellipsis or a
 * line clamp is cut on purpose, and text hidden from assistive technology, such as the editor's
 * highlight layer under its textarea, is drawn for its own component to clip. Runs in the page:
 * pass it to `page.evaluate`.
 * @returns {string[]} One line per element with cut text, empty when none is cut.
 */
export const findClipped = () => {
  const { document, getComputedStyle, NodeFilter } = globalThis;
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
  const nameOf = (element) =>
    [
      element.tagName.toLowerCase(),
      element.id === '' ? '' : `#${element.id}`,
      element.classList[0] === undefined ? '' : `.${element.classList[0]}`,
    ].join('');
  // The nearest clipping box narrower than the viewport, or undefined when the text can be scrolled
  // to, is cut on purpose, or is only clipped by the page itself, which `findOverflow` reports.
  const clipOf = (element) => {
    const text = getComputedStyle(element);
    for (let node = element; node !== null; node = node.parentElement) {
      const style = getComputedStyle(node);
      if (style.overflowX === 'auto' || style.overflowX === 'scroll') {
        return undefined;
      }
      if (style.overflowX === 'hidden' || style.overflowX === 'clip') {
        const box = node.getBoundingClientRect();
        const isOnPurpose =
          [style.textOverflow, text.textOverflow].includes('ellipsis') || style.webkitLineClamp !== 'none';
        const isVisuallyHidden = box.width <= 2 || box.height <= 2;
        const isPage = box.left <= 0.5 && box.right >= width - 0.5;
        return isOnPurpose || isVisuallyHidden || isPage ? undefined : { node, box, style };
      }
    }
    return undefined;
  };
  const found = [];
  const reported = new Set();
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let text = walker.nextNode(); text !== null; text = walker.nextNode()) {
    const element = text.parentElement;
    if (
      element === null ||
      reported.has(element) ||
      text.textContent.trim() === '' ||
      element.closest('svg, textarea, script, style, [aria-hidden="true"]') !== null
    ) {
      continue;
    }
    const range = document.createRange();
    range.selectNodeContents(text);
    const rects = [...range.getClientRects()].filter((rect) => rect.width > 0 && rect.height > 0);
    const clip = rects.length === 0 ? undefined : clipOf(element);
    if (clip === undefined) {
      continue;
    }
    const left = Math.min(...rects.map((rect) => rect.left));
    const right = Math.max(...rects.map((rect) => rect.right));
    const clipLeft = clip.box.left + Number.parseFloat(clip.style.borderLeftWidth);
    const clipRight = clip.box.right - Number.parseFloat(clip.style.borderRightWidth);
    if ((left < clipLeft - 1 || right > clipRight + 1) && !isHidden(element)) {
      reported.add(element);
      found.push(
        `${nameOf(element)} "${text.textContent.trim().slice(0, 40)}" spans ${Math.round(left)} to ${Math.round(right)} px, cut by ${nameOf(clip.node)} at ${Math.round(clipLeft)} to ${Math.round(clipRight)} px`,
      );
    }
  }
  return found;
};
