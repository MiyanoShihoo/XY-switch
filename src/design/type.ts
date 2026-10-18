export type DesignTheme = "light" | "dark";
export type UIIntent = "neutral" | "accent" | "success" | "warning" | "danger" | "info";
export type UISize = "sm" | "md" | "lg";

export const designSpacing = {
  one: "4px",
  two: "8px",
  three: "12px",
  four: "16px",
  five: "20px",
  six: "24px",
} as const;
