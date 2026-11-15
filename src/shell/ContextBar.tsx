import type { AppView } from "../lib/workspace-navigation";
import { SectionNav } from "./SideNav";

export function ContextBar({ activeView, onNavigate }: { activeView: AppView; onNavigate: (view: AppView) => void }) {
  return <SectionNav activeView={activeView} onNavigate={onNavigate} />;
}
