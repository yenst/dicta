export interface AsyncActionOptions {
  onError: (message: string, error: unknown) => void;
  fallbackMessage: string;
  onFinally?: () => void;
}

export async function runAsyncAction<T>(action: () => Promise<T>, options: AsyncActionOptions): Promise<T | undefined> {
  try {
    return await action();
  } catch (error) {
    const message = error instanceof Error && error.message ? error.message : options.fallbackMessage;
    options.onError(message, error);
    return undefined;
  } finally {
    options.onFinally?.();
  }
}
