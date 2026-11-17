// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: an `XMLHttpRequest` that answers through the stubbed `fetch` of `stubServer`, so
// an upload (files/course-files.ts `sendWithProgress`) meets the same handlers as every other
// request. Each send is recorded with its body. A held send waits for the test to report progress
// and let it finish; any other send reports its whole body sent, then answers.
import { vi } from "vitest";

type Listener = (event: ProgressEvent) => void;

class Listeners {
  private readonly listeners = new Map<string, Listener[]>();
  addEventListener(type: string, listener: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  emit(type: string, init: ProgressEventInit = {}) {
    for (const listener of this.listeners.get(type) ?? []) listener(new ProgressEvent(type, init));
  }
}

/** One upload the fake has been asked to send. */
export interface SentUpload {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: unknown;
  /** Reports `loaded` of `total` bytes sent; for a held send. */
  progress: (loaded: number, total: number) => void;
  /** Lets a held send go to the server and answer. */
  finish: () => Promise<void>;
}

export interface FakeXhrControl {
  sent: SentUpload[];
  /** Whether new sends wait for `finish`. */
  hold: boolean;
}

export function stubXhr(): FakeXhrControl {
  const control: FakeXhrControl = { sent: [], hold: false };

  class FakeXhr extends Listeners {
    readonly upload = new Listeners();
    status = 0;
    responseText = "";
    private method = "GET";
    private url = "";
    private readonly headers: Record<string, string> = {};
    private answerHeaders = "";

    open(method: string, url: string) {
      this.method = method;
      this.url = url;
    }
    setRequestHeader(key: string, value: string) {
      this.headers[key] = value;
    }
    getAllResponseHeaders() {
      return this.answerHeaders;
    }
    send(body: unknown) {
      const answer = async () => {
        try {
          const request = new Request(this.url, { method: this.method, headers: this.headers });
          const response = await globalThis.fetch(request);
          this.status = response.status;
          this.responseText = await response.text();
          this.answerHeaders = [...response.headers].map(([k, v]) => `${k}: ${v}`).join("\r\n");
          this.emit("load");
        } catch {
          this.emit("error");
        }
      };
      const sent: SentUpload = {
        method: this.method,
        url: this.url,
        headers: { ...this.headers },
        body,
        progress: (loaded, total) =>
          this.upload.emit("progress", { lengthComputable: true, loaded, total }),
        finish: answer,
      };
      control.sent.push(sent);
      if (!control.hold) {
        const size = body instanceof Blob ? body.size : 0;
        sent.progress(size, size);
        void answer();
      }
    }
  }

  vi.stubGlobal("XMLHttpRequest", FakeXhr);
  return control;
}
