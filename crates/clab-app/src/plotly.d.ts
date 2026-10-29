declare module "plotly.js-basic-dist-min" {
  const Plotly: {
    newPlot(root: HTMLElement, data: object[], layout?: object, config?: object): Promise<unknown>;
    toImage(root: HTMLElement, options: object): Promise<string>;
    relayout(root: HTMLElement, update: object): Promise<unknown>;
  };
  export default Plotly;
}
