// The parts of noVNC's RFB client this app uses; the package ships no types.
declare module '@novnc/novnc' {
  export default class RFB extends EventTarget {
    constructor(target: HTMLElement, urlOrChannel: string | object, options?: { shared?: boolean });
    scaleViewport: boolean;
    resizeSession: boolean;
    focusOnClick: boolean;
    qualityLevel: number;
    compressionLevel: number;
    disconnect(): void;
    focus(): void;
  }
}
