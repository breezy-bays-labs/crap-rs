export function safeParse(json: string): unknown {
  try {
    return JSON.parse(json);
  } catch (e) {
    return null;
  }
}

export function noHandler(): void {
  try {
    doWork();
  } finally {
    cleanup();
  }
}

declare function doWork(): void;
declare function cleanup(): void;
