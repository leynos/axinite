import {
  createMutation,
  createQuery,
  useQueryClient,
} from "@tanstack/solid-query";
import {
  type Accessor,
  createEffect,
  createMemo,
  createSignal,
  For,
  Show,
} from "solid-js";

import type { CatalogueSkillEntry, SkillInfo } from "@/lib/api/contracts";
import {
  fetchSkills,
  installSkill,
  removeSkill,
  searchSkills,
} from "@/lib/api/skills";
import { useI18n } from "@/lib/i18n/provider";

// The translate accessor mirrors the shape returned by useI18n; it is passed
// through to the extracted sub-components so translation lookups stay
// centralised on a single provider instance.
type Translate = (
  key: string,
  options?: Record<string, string | number>
) => string;

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

type SkillsSearchSectionProps = {
  t: Translate;
  query: Accessor<string>;
  setQuery: (value: string) => void;
  results: Accessor<CatalogueSkillEntry[]>;
  formatLabel: (format: string) => string;
  onInstall: (name: string) => void;
};

// Renders the catalogue search form and its result cards. Props are read
// through the props object (never destructured) so that signal reads stay
// reactive under SolidJS.
const SkillsSearchSection = (props: SkillsSearchSectionProps) => {
  return (
    <section class="catalogue-section skills-section skills-section--search">
      <div class="catalogue-section__header skills-section__header">
        <div>
          <h3 class="catalogue-section__title">
            {props.t("skills-search-title")}
          </h3>
          <p class="catalogue-section__body">{props.t("page-skills-agenda")}</p>
        </div>
      </div>

      <div class="catalogue-search skills-search">
        <div class="catalogue-form__row skills-search__row">
          <input
            class="catalogue-form__input"
            onInput={(event) => props.setQuery(event.currentTarget.value)}
            placeholder={props.t("skills-search-placeholder")}
            type="text"
            value={props.query()}
          />
          <button class="catalogue-form__button" type="button">
            {props.t("skills-search-action")}
          </button>
        </div>

        <div class="catalogue-search__results skills-search__results">
          <For each={props.results()}>
            {(result) => {
              const format = detectFormat(result.slug);
              return (
                <article class="catalogue-search__result skills-search__result">
                  <div class="catalogue-search__header">
                    <div>
                      <h4 class="catalogue-card__title">{result.name}</h4>
                      <p class="catalogue-list__body">{result.description}</p>
                    </div>
                    <div class="catalogue-detail__pills">
                      <span
                        class={FORMAT_CLASS[format] ?? "pill pill--neutral"}
                      >
                        {props.formatLabel(format)}
                      </span>
                      <span class="pill pill--neutral">{result.version}</span>
                    </div>
                  </div>
                  <p class="catalogue-search__meta">
                    {result.stars} stars · {result.downloads} downloads
                  </p>
                  <div class="catalogue-card__actions">
                    <button
                      class="catalogue-card__action"
                      type="button"
                      onClick={() => props.onInstall(result.name)}
                    >
                      {props.t("skills-action-install")}
                    </button>
                  </div>
                </article>
              );
            }}
          </For>
        </div>
      </div>
    </section>
  );
};

type SkillsInstalledSectionProps = {
  t: Translate;
  skills: Accessor<SkillInfo[]>;
  activeSkillName: Accessor<string | null>;
  setActiveSkillName: (name: string) => void;
  formatLabel: (format: string) => string;
  onRemove: () => void;
};

// Renders the grid of installed skills. The active-card class and every
// handler read through the props object so reactivity is preserved.
const SkillsInstalledSection = (props: SkillsInstalledSectionProps) => {
  return (
    <section class="catalogue-section skills-section">
      <div class="catalogue-section__header skills-section__header">
        <div>
          <h3 class="catalogue-section__title">
            {props.t("skills-installed-title")}
          </h3>
          <p class="catalogue-section__body">
            {props.t("page-skills-guardrail")}
          </p>
        </div>
      </div>

      <div class="catalogue-grid skills-grid">
        <For each={props.skills()}>
          {(skill) => {
            const format = detectFormat(skill.source);
            return (
              <article
                class={
                  props.activeSkillName() === skill.name
                    ? "catalogue-card catalogue-card--active skills-card"
                    : "catalogue-card skills-card"
                }
              >
                <div class="catalogue-card__header">
                  <div class="catalogue-card__title-wrap">
                    <h4 class="catalogue-card__title">{skill.name}</h4>
                    <span class={FORMAT_CLASS[format] ?? "pill pill--neutral"}>
                      {props.formatLabel(format)}
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
                    onClick={() => props.setActiveSkillName(skill.name)}
                    type="button"
                  >
                    {props.t("skills-action-view")}
                  </button>
                  <button
                    class="catalogue-card__action"
                    type="button"
                    onClick={() => props.setActiveSkillName(skill.name)}
                  >
                    {props.t("skills-action-disable")}
                  </button>
                  <button
                    class="catalogue-card__action"
                    type="button"
                    onClick={() => {
                      props.setActiveSkillName(skill.name);
                      props.onRemove();
                    }}
                  >
                    {props.t("skills-action-remove")}
                  </button>
                </div>
              </article>
            );
          }}
        </For>
      </div>
    </section>
  );
};

type SkillsDetailSectionProps = {
  t: Translate;
  skill: SkillInfo;
  formatLabel: (format: string) => string;
};

// Renders the detail panel for the active skill. The skill is supplied as a
// keyed value by the parent Show, so a plain prop is sufficient here.
const SkillsDetailSection = (props: SkillsDetailSectionProps) => {
  return (
    <section class="catalogue-detail skills-detail">
      <div class="catalogue-detail__header">
        <div>
          <p class="catalogue-detail__eyebrow">
            {props.t("skills-detail-eyebrow")}
          </p>
          <h3 class="catalogue-detail__title">{props.skill.name}</h3>
        </div>
        <div class="catalogue-detail__pills">
          <span
            class={
              FORMAT_CLASS[detectFormat(props.skill.source)] ??
              "pill pill--neutral"
            }
          >
            {props.formatLabel(detectFormat(props.skill.source))}
          </span>
          <span class="pill pill--neutral">{props.skill.version}</span>
        </div>
      </div>

      <div class="skills-detail__layout">
        <div class="skills-detail__body-block">
          <p class="catalogue-detail__body">{props.skill.description}</p>
          <p class="catalogue-search__meta">
            Source: {props.skill.source} · Trust: {props.skill.trust}
          </p>
        </div>

        <div class="catalogue-files skills-detail__files">
          <p class="catalogue-files__title">{props.t("skills-files-title")}</p>
          <div class="catalogue-files__list skills-files__list">
            <For each={props.skill.keywords}>
              {(keyword) => <div class="catalogue-files__item">{keyword}</div>}
            </For>
          </div>
        </div>
      </div>
    </section>
  );
};

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

        <SkillsSearchSection
          t={t}
          query={query}
          setQuery={setQuery}
          results={() => searchResults.data?.catalog ?? []}
          formatLabel={formatLabel}
          onInstall={(name) => installMutation.mutate(name)}
        />

        <SkillsInstalledSection
          t={t}
          skills={() => skills.data?.skills ?? []}
          activeSkillName={activeSkillName}
          setActiveSkillName={setActiveSkillName}
          formatLabel={formatLabel}
          onRemove={() => removeMutation.mutate()}
        />

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
            <SkillsDetailSection
              t={t}
              skill={skill}
              formatLabel={formatLabel}
            />
          )}
        </Show>
      </div>
    </section>
  );
};
