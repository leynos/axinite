import {
  createMutation,
  createQuery,
  useQueryClient,
} from "@tanstack/solid-query";
import {
  createEffect,
  createMemo,
  createSignal,
  createUniqueId,
  For,
  onCleanup,
  Show,
} from "solid-js";
import {
  AuthCard,
  GeneratedImageCard,
  JobStartCard,
} from "@/components/chat-cards";
import {
  cancelAuth,
  connectChatEvents,
  createThread,
  fetchHistory,
  fetchThreads,
  sendMessage,
  submitApproval,
  submitAuthToken,
} from "@/lib/api/chat";
import type { ImageData, ToolCallInfo } from "@/lib/api/contracts";
import { useI18n } from "@/lib/i18n/provider";
import { renderMarkdown } from "@/lib/markdown";

const MAX_IMAGES = 5;
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;

type StagedImage = {
  id: string;
  name: string;
  mediaType: string;
  data: string;
  dataUrl: string;
};

type GeneratedImage = {
  id: string;
  dataUrl: string;
  path?: string;
};

type JobCard = {
  id: string;
  jobId: string;
  title: string;
  browseUrl?: string;
};

type AuthCardState = {
  extensionName: string;
  instructions?: string;
  authUrl?: string;
  setupUrl?: string;
};

/**
 * Reads an image File as a data URL and splits it into the base64 payload and
 * media type the daemon expects (`{ media_type, data }`, no `data:` prefix).
 */
function readImageFile(
  file: File
): Promise<{ mediaType: string; data: string; dataUrl: string }> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("Unreadable image data"));
        return;
      }
      const match = /^data:([^;,]+)(?:;[^,]*)?,(.*)$/su.exec(result);
      if (!match) {
        reject(new Error("Malformed data URL"));
        return;
      }
      resolve({ mediaType: match[1], data: match[2], dataUrl: result });
    };
    reader.onerror = () =>
      reject(reader.error ?? new Error("Image read error"));
    reader.readAsDataURL(file);
  });
}

function formatTimestamp(
  value: string | null | undefined,
  fallback: string
): string {
  if (!value) {
    return fallback;
  }
  return new Intl.DateTimeFormat("en-GB", {
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

const ToolCallsSummary = (props: {
  toolCalls: ToolCallInfo[];
  label: string;
}) => {
  const [expanded, setExpanded] = createSignal(false);
  const listId = createUniqueId();

  return (
    <div class="chat-preview__tool-summary">
      <button
        aria-controls={listId}
        aria-expanded={expanded()}
        class="chat-preview__tool-summary-header"
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
      >
        <span>{props.label}</span>
        <span class="chat-preview__tool-summary-chevron" aria-hidden="true">
          {expanded() ? "\u25B4" : "\u25BE"}
        </span>
      </button>
      <Show when={expanded()}>
        <div class="chat-preview__tool-summary-list" id={listId}>
          <For each={props.toolCalls}>
            {(toolCall) => (
              <div class="chat-preview__tool-call-item">
                <span class="chat-preview__tool-call-name">
                  {toolCall.has_error ? "\u2717" : "\u2713"} {toolCall.name}
                </span>
                <Show when={toolCall.result_preview}>
                  <div class="chat-preview__tool-call-preview">
                    {toolCall.result_preview}
                  </div>
                </Show>
                <Show when={toolCall.error}>
                  <div class="chat-preview__tool-call-error">
                    {toolCall.error}
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export const ChatPreview = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [activeThreadId, setActiveThreadId] = createSignal<string>();
  const [composerText, setComposerText] = createSignal("");
  const [pendingUserMessage, setPendingUserMessage] = createSignal("");
  const [streamingResponse, setStreamingResponse] = createSignal("");
  const [isAwaitingResponse, setIsAwaitingResponse] = createSignal(false);
  const [liveStatus, setLiveStatus] = createSignal("");
  const [stagedImages, setStagedImages] = createSignal<StagedImage[]>([]);
  const [attachmentNotice, setAttachmentNotice] = createSignal("");
  const [generatedImages, setGeneratedImages] = createSignal<GeneratedImage[]>(
    []
  );
  const [jobCards, setJobCards] = createSignal<JobCard[]>([]);
  const [authCards, setAuthCards] = createSignal<AuthCardState[]>([]);
  const [authNotice, setAuthNotice] = createSignal("");
  let cardIdCounter = 0;
  const nextCardId = () => {
    cardIdCounter += 1;
    return `card-${cardIdCounter}`;
  };
  let fileInputRef: HTMLInputElement | undefined;

  const removeAuthCard = (extensionName: string) => {
    setAuthCards((prev) =>
      prev.filter((card) => card.extensionName !== extensionName)
    );
  };

  async function stageFiles(files: File[]): Promise<void> {
    setAttachmentNotice("");
    for (const file of files) {
      if (!file.type.startsWith("image/")) {
        setAttachmentNotice(t("chat-attachment-invalid-type"));
        continue;
      }
      if (file.size > MAX_IMAGE_BYTES) {
        setAttachmentNotice(
          t("chat-attachment-too-large", { name: file.name })
        );
        continue;
      }
      if (stagedImages().length >= MAX_IMAGES) {
        setAttachmentNotice(t("chat-attachment-too-many"));
        break;
      }
      try {
        const read = await readImageFile(file);
        setStagedImages((prev) =>
          prev.length >= MAX_IMAGES
            ? prev
            : [...prev, { id: nextCardId(), name: file.name, ...read }]
        );
      } catch {
        setAttachmentNotice(t("chat-attachment-invalid-type"));
      }
    }
  }

  const handleFileInput = (
    event: Event & { currentTarget: HTMLInputElement }
  ) => {
    const input = event.currentTarget;
    const files = input.files ? Array.from(input.files) : [];
    if (files.length > 0) {
      void stageFiles(files);
    }
    // Reset so selecting the same file again re-triggers the change event.
    input.value = "";
  };

  const handlePaste = (event: ClipboardEvent) => {
    const items = event.clipboardData?.items;
    if (!items) {
      return;
    }
    const files: File[] = [];
    for (const item of items) {
      if (item.kind === "file" && item.type.startsWith("image/")) {
        const file = item.getAsFile();
        if (file) {
          files.push(file);
        }
      }
    }
    if (files.length > 0) {
      event.preventDefault();
      void stageFiles(files);
    }
  };

  const handleAuthSubmit = async (
    extensionName: string,
    token: string
  ): Promise<boolean> => {
    const response = await submitAuthToken({
      extension_name: extensionName,
      token,
    });
    if (response.success) {
      removeAuthCard(extensionName);
      setAuthNotice(response.message);
      return true;
    }
    return false;
  };

  const handleAuthCancel = (extensionName: string) => {
    void cancelAuth({ extension_name: extensionName });
    removeAuthCard(extensionName);
  };

  const threads = createQuery(() => ({
    queryKey: ["chat", "threads"],
    queryFn: fetchThreads,
  }));

  const assistantThread = createMemo(
    () => threads.data?.assistant_thread ?? null
  );

  const conversationThreads = createMemo(() => threads.data?.threads ?? []);

  createEffect(() => {
    const resolvedThreadId =
      activeThreadId() ??
      threads.data?.active_thread ??
      threads.data?.assistant_thread?.id ??
      threads.data?.threads[0]?.id;
    if (resolvedThreadId && resolvedThreadId !== activeThreadId()) {
      setActiveThreadId(resolvedThreadId);
    }
  });

  const history = createQuery(() => ({
    queryKey: ["chat", "history", activeThreadId()],
    queryFn: () => fetchHistory(activeThreadId()),
    enabled: typeof activeThreadId() === "string",
  }));

  createEffect(() => {
    const pending = pendingUserMessage();
    if (!pending) {
      return;
    }

    const latestTurn = history.data?.turns.at(-1);
    if (latestTurn?.user_input === pending) {
      setPendingUserMessage("");
    }
  });

  const createThreadMutation = createMutation(() => ({
    mutationFn: createThread,
    onSuccess: (thread) => {
      setActiveThreadId(thread.id);
      void queryClient.invalidateQueries({ queryKey: ["chat", "threads"] });
      void queryClient.invalidateQueries({ queryKey: ["chat", "history"] });
    },
  }));

  const sendMutation = createMutation(() => ({
    mutationFn: (content: string) => {
      const images: ImageData[] = stagedImages().map((image) => ({
        media_type: image.mediaType,
        data: image.data,
      }));
      return sendMessage({
        content,
        thread_id: activeThreadId() ?? null,
        timezone:
          Intl.DateTimeFormat().resolvedOptions().timeZone ?? "Europe/London",
        images,
      });
    },
    onMutate: (content) => {
      setPendingUserMessage(content);
      setComposerText("");
      setStreamingResponse("");
      setIsAwaitingResponse(true);
      setLiveStatus(t("chat-status-waiting"));
    },
    onSuccess: () => {
      setStagedImages([]);
      setAttachmentNotice("");
      setLiveStatus(t("chat-status-streaming"));
      void queryClient.invalidateQueries({ queryKey: ["chat", "history"] });
      void queryClient.invalidateQueries({ queryKey: ["chat", "threads"] });
    },
    onError: (_error, content) => {
      setComposerText(content);
      setPendingUserMessage("");
      setStreamingResponse("");
      setIsAwaitingResponse(false);
      setLiveStatus(t("chat-status-failed"));
    },
  }));

  const approvalMutation = createMutation(() => ({
    mutationFn: (action: "approve" | "deny") =>
      submitApproval({
        request_id: history.data?.pending_approval?.request_id ?? "",
        action,
        thread_id: activeThreadId() ?? null,
      }),
    onSuccess: () => {
      setStreamingResponse("");
      void queryClient.invalidateQueries({ queryKey: ["chat", "history"] });
      void queryClient.invalidateQueries({ queryKey: ["chat", "threads"] });
    },
  }));

  createEffect(() => {
    const source = connectChatEvents((event) => {
      if (
        "thread_id" in event &&
        event.thread_id &&
        event.thread_id !== activeThreadId()
      ) {
        return;
      }

      switch (event.type) {
        case "thinking":
        case "status":
          setLiveStatus(event.message);
          break;
        case "tool_started":
          setLiveStatus(t("chat-status-tool-running", { name: event.name }));
          break;
        case "tool_result":
          setLiveStatus(event.preview);
          break;
        case "tool_completed":
          setLiveStatus(
            event.success
              ? t("chat-status-tool-success", { name: event.name })
              : (event.error ??
                  t("chat-status-tool-failed", { name: event.name }))
          );
          break;
        case "stream_chunk":
          setIsAwaitingResponse(false);
          setStreamingResponse((current) => `${current}${event.content}`);
          break;
        case "response":
          setPendingUserMessage("");
          setIsAwaitingResponse(false);
          setStreamingResponse("");
          setLiveStatus(t("chat-status-complete"));
          void queryClient.invalidateQueries({ queryKey: ["chat", "history"] });
          void queryClient.invalidateQueries({ queryKey: ["chat", "threads"] });
          break;
        case "approval_needed":
          setPendingUserMessage("");
          setIsAwaitingResponse(false);
          setLiveStatus(event.description);
          void queryClient.invalidateQueries({ queryKey: ["chat", "history"] });
          break;
        case "error":
          setPendingUserMessage("");
          setIsAwaitingResponse(false);
          setLiveStatus(event.message);
          break;
        case "image_generated":
          setGeneratedImages((prev) => [
            ...prev,
            {
              id: nextCardId(),
              dataUrl: event.data_url,
              path: event.path,
            },
          ]);
          break;
        case "job_started":
          setJobCards((prev) => [
            ...prev,
            {
              id: nextCardId(),
              jobId: event.job_id,
              title: event.title,
              browseUrl: event.browse_url,
            },
          ]);
          break;
        case "auth_required":
          setAuthCards((prev) => [
            // Dedupe by extension: a fresh prompt replaces any existing card.
            ...prev.filter(
              (card) => card.extensionName !== event.extension_name
            ),
            {
              extensionName: event.extension_name,
              instructions: event.instructions,
              authUrl: event.auth_url,
              setupUrl: event.setup_url,
            },
          ]);
          break;
        case "auth_completed":
          removeAuthCard(event.extension_name);
          setAuthNotice(event.message);
          break;
        case "heartbeat":
        case "extension_status":
        case "job_message":
        case "job_tool_use":
        case "job_tool_result":
        case "job_status":
        case "job_result":
          break;
      }
    });

    onCleanup(() => source.close());
  });

  return (
    <section class="route-preview route-preview--chat">
      <div aria-hidden="true" class="route-preview__watermark">
        {t("chat-watermark")}
      </div>
      <div class="route-preview__layout route-preview__layout--chat">
        <aside class="route-sidebar route-sidebar--chat">
          <div class="route-sidebar__toolbar">
            <button
              class="route-sidebar__icon-button"
              type="button"
              onClick={() => createThreadMutation.mutate()}
            >
              +
            </button>
            <div class="route-sidebar__spacer" />
            <button class="route-sidebar__icon-button" type="button">
              {"<"}
            </button>
          </div>

          <Show when={assistantThread()}>
            {(thread) => (
              <button
                class={
                  activeThreadId() === thread().id
                    ? "route-sidebar__list-item route-sidebar__list-item--active"
                    : "route-sidebar__list-item"
                }
                onClick={() => {
                  setActiveThreadId(thread().id);
                  setStreamingResponse("");
                }}
                type="button"
              >
                <span class="route-sidebar__list-label">
                  {thread().title ?? t("route-chat-label")}
                </span>
                <span class="route-sidebar__list-time">
                  {formatTimestamp(thread().updated_at, t("timestamp-pending"))}
                </span>
              </button>
            )}
          </Show>

          <Show when={conversationThreads().length > 0}>
            <div class="route-sidebar__section-header">
              <span>{t("chat-sidebar-conversations")}</span>
            </div>
          </Show>

          <div class="route-sidebar__session-list">
            <For each={conversationThreads()}>
              {(thread) => (
                <button
                  class={
                    activeThreadId() === thread.id
                      ? "route-sidebar__list-item route-sidebar__list-item--active"
                      : "route-sidebar__list-item"
                  }
                  onClick={() => {
                    setActiveThreadId(thread.id);
                    setStreamingResponse("");
                  }}
                  type="button"
                >
                  <span class="route-sidebar__list-label">
                    {thread.title ?? thread.id}
                  </span>
                  <span class="route-sidebar__list-time">
                    {formatTimestamp(thread.updated_at, t("timestamp-pending"))}
                  </span>
                </button>
              )}
            </For>
          </div>
        </aside>

        <main class="chat-preview__main">
          <div class="chat-preview__scroll">
            <div class="chat-preview__conversation">
              <For each={history.data?.turns ?? []}>
                {(turn) => (
                  <>
                    <div class="chat-preview__turn chat-preview__turn--user">
                      <div class="chat-preview__bubble chat-preview__bubble--user">
                        <p>{turn.user_input}</p>
                      </div>
                    </div>
                    <Show when={turn.tool_calls.length > 0}>
                      <ToolCallsSummary
                        toolCalls={turn.tool_calls}
                        label={t("chat-tools-used", {
                          count: turn.tool_calls.length,
                        })}
                      />
                    </Show>
                    <Show when={turn.response != null}>
                      <div class="chat-preview__turn chat-preview__turn--assistant">
                        <div class="chat-preview__bubble chat-preview__bubble--assistant">
                          <div
                            class="chat-preview__markdown"
                            innerHTML={renderMarkdown(turn.response ?? "")}
                          />
                        </div>
                      </div>
                    </Show>
                    <Show
                      when={
                        turn.response == null && turn.tool_calls.length === 0
                      }
                    >
                      <div class="chat-preview__turn chat-preview__turn--assistant">
                        <div class="chat-preview__bubble chat-preview__bubble--assistant">
                          <div class="chat-preview__markdown">
                            <p>{t("chat-response-pending")}</p>
                          </div>
                        </div>
                      </div>
                    </Show>
                  </>
                )}
              </For>

              <Show when={pendingUserMessage().length > 0}>
                <div class="chat-preview__turn chat-preview__turn--user">
                  <div class="chat-preview__bubble chat-preview__bubble--user">
                    <p>{pendingUserMessage()}</p>
                  </div>
                </div>
              </Show>

              <Show
                when={isAwaitingResponse() && streamingResponse().length === 0}
              >
                <div class="chat-preview__turn chat-preview__turn--assistant">
                  <div class="chat-preview__bubble chat-preview__bubble--assistant">
                    <div
                      aria-busy="true"
                      aria-live="polite"
                      class="chat-preview__spinner-row"
                    >
                      <span aria-hidden="true" class="chat-preview__spinner" />
                      <p>{liveStatus() || t("chat-status-waiting")}</p>
                    </div>
                  </div>
                </div>
              </Show>

              <Show when={streamingResponse().length > 0}>
                <div class="chat-preview__turn chat-preview__turn--assistant">
                  <div class="chat-preview__bubble chat-preview__bubble--assistant">
                    <div class="chat-preview__markdown">
                      <p>{streamingResponse()}</p>
                    </div>
                  </div>
                </div>
              </Show>

              <Show when={history.data?.pending_approval}>
                {(approval) => (
                  <div class="chat-preview__turn chat-preview__turn--assistant">
                    <div class="chat-preview__bubble chat-preview__bubble--assistant">
                      <div class="chat-preview__markdown">
                        <h3>{approval().tool_name}</h3>
                        <p>{approval().description}</p>
                        <p>{approval().parameters}</p>
                        <div class="dashboard-detail__actions">
                          <button
                            class="dashboard-detail__ghost"
                            type="button"
                            onClick={() => approvalMutation.mutate("approve")}
                          >
                            {t("chat-approval-approve")}
                          </button>
                          <button
                            class="dashboard-detail__ghost"
                            type="button"
                            onClick={() => approvalMutation.mutate("deny")}
                          >
                            {t("chat-approval-deny")}
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                )}
              </Show>

              <For each={generatedImages()}>
                {(image) => (
                  <GeneratedImageCard
                    dataUrl={image.dataUrl}
                    path={image.path}
                  />
                )}
              </For>

              <For each={jobCards()}>
                {(job) => (
                  <JobStartCard
                    browseUrl={job.browseUrl}
                    jobId={job.jobId}
                    title={job.title}
                  />
                )}
              </For>

              <For each={authCards()}>
                {(card) => (
                  <AuthCard
                    authUrl={card.authUrl}
                    extensionName={card.extensionName}
                    instructions={card.instructions}
                    onCancel={() => handleAuthCancel(card.extensionName)}
                    onSubmit={(token) =>
                      handleAuthSubmit(card.extensionName, token)
                    }
                    setupUrl={card.setupUrl}
                  />
                )}
              </For>

              <Show when={authNotice().length > 0}>
                <div class="chat-preview__auth-notice" role="status">
                  <span>{authNotice()}</span>
                  <button
                    class="chat-preview__auth-notice-dismiss"
                    type="button"
                    onClick={() => setAuthNotice("")}
                  >
                    {t("chat-auth-notice-dismiss")}
                  </button>
                </div>
              </Show>
            </div>
          </div>

          <div class="chat-preview__composer-wrap">
            <div class="chat-preview__composer-shell">
              <Show when={liveStatus().length > 0}>
                <p class="chat-preview__safety-note">{liveStatus()}</p>
              </Show>
              <Show when={attachmentNotice().length > 0}>
                <p class="chat-preview__attachment-notice" role="alert">
                  {attachmentNotice()}
                </p>
              </Show>
              <Show when={stagedImages().length > 0}>
                <ul
                  aria-label={t("chat-attachment-strip-label")}
                  class="chat-preview__attachments"
                >
                  <For each={stagedImages()}>
                    {(image) => (
                      <li class="chat-preview__attachment">
                        <img
                          alt={image.name}
                          class="chat-preview__attachment-thumb"
                          src={image.dataUrl}
                        />
                        <button
                          aria-label={t("chat-attachment-remove", {
                            name: image.name,
                          })}
                          class="chat-preview__attachment-remove"
                          type="button"
                          onClick={() =>
                            setStagedImages((prev) =>
                              prev.filter((staged) => staged.id !== image.id)
                            )
                          }
                        >
                          {"✕"}
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
              <div class="chat-preview__composer">
                <input
                  accept="image/*"
                  class="chat-preview__file-input"
                  hidden
                  multiple
                  onChange={handleFileInput}
                  ref={fileInputRef}
                  type="file"
                />
                <textarea
                  aria-label={t("chat-composer-label")}
                  class="chat-preview__textarea"
                  onInput={(event) =>
                    setComposerText(event.currentTarget.value)
                  }
                  onPaste={handlePaste}
                  placeholder={t("chat-composer-placeholder")}
                  rows={1}
                  value={composerText()}
                />
                <div class="chat-preview__composer-actions">
                  <button
                    aria-label={t("chat-attach-images")}
                    class="chat-preview__ghost-button"
                    type="button"
                    onClick={() => fileInputRef?.click()}
                  >
                    +
                  </button>
                  <button
                    class="chat-preview__send-button"
                    disabled={
                      composerText().trim().length === 0 &&
                      stagedImages().length === 0
                    }
                    type="button"
                    onClick={() => sendMutation.mutate(composerText().trim())}
                  >
                    {t("chat-send-button")}
                  </button>
                </div>
              </div>
              <p class="chat-preview__safety-note">{t("chat-safety-note")}</p>
            </div>
          </div>
        </main>
      </div>
    </section>
  );
};
