import { GlobalRegistrator } from "@happy-dom/global-registrator";
GlobalRegistrator.register();

// bun prints a failed expectation's DOM node with its whole happy-dom object graph (megabytes, seconds per failure,
// and waitFor fails its callback many times): print the markup instead.
const { prettyDOM } = await import("@testing-library/preact");
Object.defineProperty(Node.prototype, Bun.inspect.custom, {
  value(this: Node) {
    if (this instanceof Element || this instanceof Document) return prettyDOM(this, undefined, { highlight: false });
    return `${this.nodeName} ${JSON.stringify(this.textContent)}`;
  },
});
