import {
  createMutation,
  createQuery,
  useQueryClient,
} from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";

import {
  fetchSkills,
  installSkill,
  removeSkill,
  searchSkills,
} from "@/lib/api/skills";
import { useI18n } from "@/lib/i18n/provider";

const FORMAT_CLASS: Record<string, string> = {
  bundle: "pill pill--violet",
  single: "pill pill--neutral",
  preview: "pill pill--warning",
};

function detectFormat(source: string): string {
  if (source.includes("bundle")) {
    return "bundle";
  }
  if (source.includes("catalog")) {
    return "single";
  }
  return "preview";
}

export const SkillsPreview = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [activeSkillName, setActiveSkillName] = createSignal<string | null>(
    null
  );
  const [query, setQuery] = createSignal("");
  const [urlName, setUrlName] = createSignal("");
  const [urlValue, setUrlValue] = createSignal("");
  const [inlineName, setInlineName] = createSignal("");

  const skills = createQuery(() => ({
    queryKey: ["skills", "list"],
    queryFn: fetchSkills,
  }));

  createEffect(() => {
    const firstSkill = skills.data?.skills[0]?.name ?? null;
    if (activeSkillName() === null && firstSkill) {
      setActiveSkillName(firstSkill);
    }
  });

  const searchResults = createQuery(() => ({
    queryKey: ["skills", "search", query().trim()],
    queryFn: () => searchSkills({ query: query().trim() }),
    enabled: query().trim().length > 0,
  }));

  const activeSkill = createMemo(
    () =>
      skills.data?.skills.find((skill) => skill.name === activeSkillName()) ??
      null
  );

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["skills"] });
  };

  const installMutation = createMutation(() => ({
    mutationFn: (name: string) =>
      installSkill({
        name,
        slug: `catalog/${name}`,
      }),
    onSuccess: refresh,
  }));

  const urlInstallMutation = createMutation(() => ({
    mutationFn: () =>
      installSkill({
        name: urlName().trim() || "remote_skill",
        url: urlValue().trim(),
      }),
    onSuccess: () => {
      setUrlName("");
      setUrlValue("");
      refresh();
    },
  }));

  const inlineInstallMutation = createMutation(() => ({
    mutationFn: () =>
      installSkill({
        name: inlineName().trim() || "uploaded_skill",
        content: `# ${inlineName().trim() || "uploaded_skill"}\n\nMock uploaded skill content.`,
      }),
    onSuccess: () => {
      setInlineName("");
      refresh();
    },
  }));

  const removeMutation = createMutation(() => ({
    mutationFn: () => removeSkill(activeSkillName() ?? ""),
    onSuccess: () => {
      refresh();
      setActiveSkillName(null);
    },
  }));

  const formatLabel = (format: string) => {
    if (format === "bundle") {
      return t("skills-format-bundle");
    }
    if (format === "single") {
      return t("skills-format-single");
    }
    return t("skills-format-preview");
  };

  return (
    <section class="route-preview route-preview--catalogue route-preview--skills">
      <div aria-hidden="true" class="route-preview__watermark">
        {t("skills-watermark")}
      </div>

      <div class="catalogue-preview catalogue-preview--skills">
        <header class="route-preview__intro catalogue-preview__intro skills-preview__intro">
          <h2 class="route-preview__title">{t("route-skills-label")}</h2>
          <p class="route-preview__summary">{t("page-skills-summary")}</p>
        </header>

        <section class="catalogue-section skills-section skills-section--search">
          <div class="catalogue-section__header skills-section__header">
            <div>
              <h3 class="catalogue-section__title">
                {t("skills-search-title")}
              </h3>
              <p class="catalogue-section__body">{t("page-skills-agenda")}</p>
            </div>
          </div>

          <div class="catalogue-search skills-search">
            <div class="catalogue-form__row skills-search__row">
              <input
                class="catalogue-form__input"
                onInput={(event) => setQuery(event.currentTarget.value)}
                placeholder={t("skills-search-placeholder")}
                type="text"
                value={query()}
              />
              <button class="catalogue-form__button" type="button">
                {t("skills-search-action")}
              </button>
            </div>

            <div class="catalogue-search__results skills-search__results">
              <For each={searchResults.data?.catalog ?? []}>
                {(result) => {
                  const format = detectFormat(result.slug);
                  return (
                    <article class="catalogue-search__result skills-search__result">
                      <div class="catalogue-search__header">
                        <div>
                          <h4 class="catalogue-card__title">{result.name}</h4>
                          <p class="catalogue-list__body">
                            {result.description}
                          </p>
                        </div>
                        <div class="catalogue-detail__pills">
                          <span
                            class={FORMAT_CLASS[format] ?? "pill pill--neutral"}
                          >
                            {formatLabel(format)}
                          </span>
                          <span class="pill pill--neutral">
                            {result.version}
                          </span>
                        </div>
                      </div>
                      <p class="catalogue-search__meta">
                        {result.stars} stars · {result.downloads} downloads
                      </p>
                      <div class="catalogue-card__actions">
                        <button
                          class="catalogue-card__action"
                          type="button"
                          onClick={() => installMutation.mutate(result.name)}
                        >
                          {t("skills-action-install")}
                        </button>
                      </div>
                    </article>
                  );
                }}
              </For>
            </div>
          </div>
        </section>

        <section class="catalogue-section skills-section">
          <div class="catalogue-section__header skills-section__header">
            <div>
              <h3 class="catalogue-section__title">
                {t("skills-installed-title")}
              </h3>
              <p class="catalogue-section__body">
                {t("page-skills-guardrail")}
              </p>
            </div>
          </div>

          <div class="catalogue-grid skills-grid">
            <For each={skills.data?.skills ?? []}>
              {(skill) => {
                const format = detectFormat(skill.source);
                return (
                  <article
                    class={
                      activeSkillName() === skill.name
                        ? "catalogue-card catalogue-card--active skills-card"
                        : "catalogue-card skills-card"
                    }
                  >
                    <div class="catalogue-card__header">
                      <div class="catalogue-card__title-wrap">
                        <h4 class="catalogue-card__title">{skill.name}</h4>
                        <span
                          class={FORMAT_CLASS[format] ?? "pill pill--neutral"}
                        >
                          {formatLabel(format)}
                        </span>
                      </div>
                      <div class="catalogue-card__meta">
                        <span>{skill.version}</span>
                        <span class="catalogue-status-dot catalogue-status-dot--active" />
                      </div>
                    </div>

                    <p class="catalogue-card__body">{skill.description}</p>
                    <p class="catalogue-search__meta">
                      {skill.keywords.join(", ")}
                    </p>

                    <div class="catalogue-card__actions">
                      <button
                        class="catalogue-card__action"
                        onClick={() => setActiveSkillName(skill.name)}
                        type="button"
                      >
                        {t("skills-action-view")}
                      </button>
                      <button
                        class="catalogue-card__action"
                        type="button"
                        onClick={() => setActiveSkillName(skill.name)}
                      >
                        {t("skills-action-disable")}
                      </button>
                      <button
                        class="catalogue-card__action"
                        type="button"
                        onClick={() => {
                          setActiveSkillName(skill.name);
                          removeMutation.mutate();
                        }}
                      >
                        {t("skills-action-remove")}
                      </button>
                    </div>
                  </article>
                );
              }}
            </For>
          </div>
        </section>

        <div class="catalogue-panel-grid skills-panel-grid">
          <section class="catalogue-panel skills-panel">
            <div class="catalogue-panel__mark">{t("skills-url-mark")}</div>
            <div class="catalogue-panel__content">
              <h3 class="catalogue-panel__title">{t("skills-url-title")}</h3>

              <div class="catalogue-form">
                <label class="catalogue-form__label" for="skills-url-name">
                  {t("skills-url-name-label")}
                </label>
                <input
                  class="catalogue-form__input"
                  id="skills-url-name"
                  onInput={(event) => setUrlName(event.currentTarget.value)}
                  placeholder={t("skills-url-name-placeholder")}
                  type="text"
                  value={urlName()}
                />
              </div>

              <div class="catalogue-form">
                <label class="catalogue-form__label" for="skills-url-input">
                  {t("skills-url-field-label")}
                </label>
                <div class="catalogue-form__row skills-search__row">
                  <input
                    class="catalogue-form__input"
                    id="skills-url-input"
                    onInput={(event) => setUrlValue(event.currentTarget.value)}
                    placeholder={t("skills-url-placeholder")}
                    type="text"
                    value={urlValue()}
                  />
                  <button
                    class="catalogue-form__button"
                    type="button"
                    onClick={() => urlInstallMutation.mutate()}
                    disabled={urlValue().trim().length === 0}
                  >
                    {t("skills-url-action")}
                  </button>
                </div>
              </div>

              <p class="catalogue-panel__hint">{t("skills-url-hint")}</p>
            </div>
          </section>

          <section class="catalogue-panel skills-panel">
            <div class="catalogue-panel__mark">{t("skills-upload-mark")}</div>
            <div class="catalogue-panel__content">
              <h3 class="catalogue-panel__title">{t("skills-upload-title")}</h3>

              <button class="catalogue-upload" type="button">
                <span class="catalogue-upload__title">
                  {t("skills-upload-drop-title")}
                </span>
                <span class="catalogue-upload__body">
                  {t("skills-upload-drop-body")}
                </span>
                <span class="catalogue-upload__meta">
                  {t("skills-upload-drop-meta")}
                </span>
              </button>

              <div class="catalogue-form__row skills-search__row">
                <input
                  class="catalogue-form__input"
                  onInput={(event) => setInlineName(event.currentTarget.value)}
                  placeholder={t("skills-upload-name-placeholder")}
                  type="text"
                  value={inlineName()}
                />
                <button
                  class="catalogue-form__button"
                  type="button"
                  onClick={() => inlineInstallMutation.mutate()}
                >
                  {t("skills-upload-action")}
                </button>
              </div>

              <p class="catalogue-panel__hint">{t("skills-upload-hint")}</p>
            </div>
          </section>
        </div>

        <Show keyed when={activeSkill()}>
          {(skill) => (
            <section class="catalogue-detail skills-detail">
              <div class="catalogue-detail__header">
                <div>
                  <p class="catalogue-detail__eyebrow">
                    {t("skills-detail-eyebrow")}
                  </p>
                  <h3 class="catalogue-detail__title">{skill.name}</h3>
                </div>
                <div class="catalogue-detail__pills">
                  <span
                    class={
                      FORMAT_CLASS[detectFormat(skill.source)] ??
                      "pill pill--neutral"
                    }
                  >
                    {formatLabel(detectFormat(skill.source))}
                  </span>
                  <span class="pill pill--neutral">{skill.version}</span>
                </div>
              </div>

              <div class="skills-detail__layout">
                <div class="skills-detail__body-block">
                  <p class="catalogue-detail__body">{skill.description}</p>
                  <p class="catalogue-search__meta">
                    Source: {skill.source} · Trust: {skill.trust}
                  </p>
                </div>

                <div class="catalogue-files skills-detail__files">
                  <p class="catalogue-files__title">
                    {t("skills-files-title")}
                  </p>
                  <div class="catalogue-files__list skills-files__list">
                    <For each={skill.keywords}>
                      {(keyword) => (
                        <div class="catalogue-files__item">{keyword}</div>
                      )}
                    </For>
                  </div>
                </div>
              </div>
            </section>
          )}
        </Show>
      </div>
    </section>
  );
};
