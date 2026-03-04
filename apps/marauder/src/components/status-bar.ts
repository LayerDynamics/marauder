/**
 * Status bar component — vanilla TS, manages #status-bar DOM element.
 */

export class StatusBar {
  private leftEl: HTMLElement;
  private centerEl: HTMLElement;
  private rightEl: HTMLElement;

  constructor(container: HTMLElement) {
    this.leftEl = document.createElement("div");
    this.leftEl.className = "status-left";
    container.appendChild(this.leftEl);

    this.centerEl = document.createElement("div");
    this.centerEl.className = "status-center";
    container.appendChild(this.centerEl);

    this.rightEl = document.createElement("div");
    this.rightEl.className = "status-right";
    container.appendChild(this.rightEl);
  }

  setCwd(path: string): void {
    this.leftEl.textContent = path;
  }

  setCommand(cmd: string): void {
    this.centerEl.textContent = cmd;
  }

  clearCommand(): void {
    this.centerEl.textContent = "";
  }

  setDimensions(rows: number, cols: number): void {
    this.rightEl.textContent = `${cols}\u00d7${rows}`;
  }
}
