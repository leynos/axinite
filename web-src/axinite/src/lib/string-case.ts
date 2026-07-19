export function capitalise(value: string): string {
  if (value.length === 0) {
    return value;
  }

  return `${value[0]?.toUpperCase() ?? ""}${value.slice(1)}`;
}

export function pascalCase(value: string): string {
  return value
    .split("_")
    .map((segment) => capitalise(segment))
    .join("");
}
