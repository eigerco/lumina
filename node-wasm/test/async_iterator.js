export async function drain_async_iterator(iterator) {
  var items = [];
  for await (const item of iterator) {
    console.log("[JS] item: ", item);
    items.push(item);
  }
  return items;
}
