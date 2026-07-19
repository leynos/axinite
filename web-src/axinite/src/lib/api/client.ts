type RequestOptions = {
  headers?: HeadersInit;
  method?: string;
  body?: BodyInit | null;
};

async function request<T>(
  url: string,
  options: RequestOptions = {}
): Promise<T> {
  const timeoutMs = 5_000;
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(url, {
      ...options,
      signal: controller.signal,
      headers: {
        Accept: "application/json",
        ...(options.headers ?? {}),
      },
    });

    if (!response.ok) {
      const message = await response.text();
      throw new Error(
        message.length > 0
          ? message
          : `Request failed for ${url} with ${response.status}`
      );
    }

    if (response.status === 204) {
      return undefined as T;
    }

    return (await response.json()) as T;
  } catch (error) {
    if (controller.signal.aborted) {
      throw new Error(`Request timed out after ${timeoutMs}ms`);
    }
    throw error;
  } finally {
    clearTimeout(timeoutId);
  }
}

export function requestJson<T>(url: string): Promise<T> {
  return request<T>(url);
}

export function postJson<T>(
  url: string,
  body?: unknown,
  headers?: HeadersInit
): Promise<T> {
  return request<T>(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(headers ?? {}),
    },
    body: typeof body === "undefined" ? null : JSON.stringify(body),
  });
}

export function putJson<T>(
  url: string,
  body?: unknown,
  headers?: HeadersInit
): Promise<T> {
  return request<T>(url, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      ...(headers ?? {}),
    },
    body: typeof body === "undefined" ? null : JSON.stringify(body),
  });
}

export function deleteJson<T>(url: string): Promise<T> {
  return request<T>(url, {
    method: "DELETE",
  });
}

export function createEventStream(url: string): EventSource {
  if (typeof EventSource === "undefined") {
    const stub = {
      onerror: null,
      onmessage: null,
      onopen: null,
      readyState: 0,
      url,
      withCredentials: false,
      addEventListener() {},
      removeEventListener() {},
      dispatchEvent() {
        return true;
      },
      close() {},
    };

    return stub as unknown as EventSource;
  }

  return new EventSource(url, { withCredentials: false });
}
