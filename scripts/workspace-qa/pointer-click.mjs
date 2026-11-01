export async function dispatchElementClick({ evaluate, call }, expression) {
  const point = await evaluate(`(async () => {
    await new Promise(resolve => requestAnimationFrame(resolve));
    const element = (${expression});
    if (!element) throw new Error("Missing click target");
    if (element.disabled) throw new Error("Disabled click target");
    element.scrollIntoView({ behavior: "instant", block: "center", inline: "nearest" });
    await new Promise(resolve => requestAnimationFrame(resolve));
    if (!element.isConnected) throw new Error("Detached click target");
    if (element.disabled) throw new Error("Disabled click target");
    const bounds = element.getBoundingClientRect();
    const x = bounds.x + bounds.width / 2;
    const y = bounds.y + bounds.height / 2;
    if (bounds.width <= 0 || bounds.height <= 0 || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) throw new Error("Click target is outside the viewport");
    const hit = document.elementFromPoint(x, y);
    if (!hit || !element.contains(hit)) throw new Error("Click target is obscured");
    return { x, y };
  })()`);
  await call('Input.dispatchMouseEvent', { type: 'mousePressed', button: 'left', clickCount: 1, ...point });
  await call('Input.dispatchMouseEvent', { type: 'mouseReleased', button: 'left', clickCount: 1, ...point });
}
