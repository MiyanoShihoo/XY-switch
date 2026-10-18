import { useCallback, useState } from "react";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  closestCenter,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  sortableKeyboardCoordinates,
} from "@dnd-kit/sortable";
import type { Provider } from "../types/provider";

export function useDragSort(
  providers: Provider[],
  onSaveOrder: (ordered: Provider[]) => Promise<void>
) {
  const [activeId, setActiveId] = useState<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    })
  );

  const handleDragStart = useCallback((event: any) => {
    setActiveId(event.active.id as string);
  }, []);

  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      setActiveId(null);
      const { active, over } = event;

      if (over && active.id !== over.id) {
        const oldIndex = providers.findIndex((p) => p.id === active.id);
        const newIndex = providers.findIndex((p) => p.id === over.id);

        if (oldIndex !== -1 && newIndex !== -1) {
          const newOrder = arrayMove(providers, oldIndex, newIndex);
          await onSaveOrder(newOrder);
        }
      }
    },
    [providers, onSaveOrder]
  );

  return {
    sensors,
    handleDragStart,
    handleDragEnd,
    activeId,
    closestCenter,
  };
}