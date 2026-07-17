import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useI18n } from "../../i18n";
import { cn } from "@/lib/utils";

export type SelectValue = string | number;

export type SelectOption<T extends SelectValue = SelectValue> = {
  value: T;
  label: string;
  disabled?: boolean;
  providerId?: string;
  // 新增：供应商图标名（用于分组下拉中渲染图标）
  icon?: string;
  // 新增：简短说明
  description?: string;
  // 新增：徽章文案，如"订阅" / "余额" / "网关"
  badge?: string;
};

/** 分组下拉中的一组选项 */
export interface AppSelectGroup<T extends SelectValue = SelectValue> {
  /** 分组标题，如"官方订阅" */
  label: string;
  options: Array<SelectOption<T>>;
}

/** 面板渲染条目：分组标题或可选选项 */
type PanelEntry<T extends SelectValue = SelectValue> =
  | { kind: "group"; label: string }
  | { kind: "option"; option: SelectOption<T>; optionIndex: number };

type AppSelectProps<T extends SelectValue = SelectValue> = {
  modelValue: T | null;
  // 扁平模式（与 groups 互斥，二者至少传一个）
  options?: Array<SelectOption<T>>;
  // 分组模式（与 options 互斥）
  groups?: Array<AppSelectGroup<T>>;
  placeholder?: string;
  disabled?: boolean;
  ariaLabel?: string;
  listMaxHeight?: number;
  className?: string;
  // 触发器附加类名（用于紧凑尺寸等调用方定制）
  triggerClassName?: string;
  onChange: (value: T) => void;
  onOpen?: () => void;
  onClose?: () => void;
  renderSelected?: (option: SelectOption<T> | null) => ReactNode;
  renderOption?: (args: {
    option: SelectOption<T>;
    selected: boolean;
    active: boolean;
  }) => ReactNode;
};

export default function AppSelect<T extends SelectValue = SelectValue>({
  modelValue,
  options,
  groups,
  placeholder,
  disabled = false,
  ariaLabel,
  listMaxHeight = 240,
  className,
  triggerClassName,
  onChange,
  onOpen,
  onClose,
  renderSelected,
  renderOption,
}: AppSelectProps<T>) {
  const { t } = useI18n();
  const [isOpen, setIsOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [panelStyle, setPanelStyle] = useState<CSSProperties>({});
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);

  // 归一化为扁平 options（分组模式下从 groups 展开）
  const flatOptions: Array<SelectOption<T>> = useMemo(() => {
    if (groups && groups.length > 0) {
      return groups.flatMap((group) => group.options);
    }
    return options ?? [];
  }, [groups, options]);

  // 面板渲染条目（含分组标题分隔符）
  const panelEntries: Array<PanelEntry<T>> = useMemo(() => {
    if (groups && groups.length > 0) {
      const entries: Array<PanelEntry<T>> = [];
      let optionIndex = 0;
      for (const group of groups) {
        if (group.options.length === 0) {
          continue;
        }
        entries.push({ kind: "group", label: group.label });
        for (const option of group.options) {
          entries.push({ kind: "option", option, optionIndex });
          optionIndex += 1;
        }
      }
      return entries;
    }
    return (options ?? []).map((option, optionIndex) => ({
      kind: "option" as const,
      option,
      optionIndex,
    }));
  }, [groups, options]);

  const hasAnyOption = flatOptions.length > 0;
  const selectedOption = flatOptions.find((option) => option.value === modelValue) ?? null;
  const resolvedPlaceholder = placeholder ?? t("common.select");
  const resolvedAriaLabel = ariaLabel ?? t("common.selectOption");

  function closeMenu() {
    if (!isOpen) {
      return;
    }

    setIsOpen(false);
    onClose?.();
  }

  function getFirstEnabledIndex() {
    return flatOptions.findIndex((option) => !option.disabled);
  }

  function setActiveToSelected() {
    const selectedIndex = flatOptions.findIndex((option) => option.value === modelValue && !option.disabled);
    setActiveIndex(selectedIndex >= 0 ? selectedIndex : getFirstEnabledIndex());
  }

  function syncPanelPosition() {
    const trigger = triggerRef.current;
    const panel = panelRef.current;
    if (!trigger || !panel) {
      return;
    }

    const rect = trigger.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    const panelHeight = Math.min(panel.scrollHeight, listMaxHeight);
    const gap = 6;
    const edgePadding = 12;
    const spaceBelow = viewportHeight - rect.bottom - edgePadding;
    const placeAbove = spaceBelow < Math.min(panelHeight, 180) && rect.top > spaceBelow;
    const top = placeAbove
      ? Math.max(edgePadding, rect.top - panelHeight - gap)
      : Math.max(edgePadding, Math.min(viewportHeight - edgePadding - panelHeight, rect.bottom + gap));
    const maxLeft = Math.max(edgePadding, viewportWidth - rect.width - edgePadding);
    const left = Math.min(Math.max(edgePadding, rect.left), maxLeft);

    setPanelStyle({
      top: `${top}px`,
      left: `${left}px`,
      width: `${rect.width}px`,
      maxHeight: `${listMaxHeight}px`,
    });
  }

  function scrollActiveOptionIntoView() {
    const panel = panelRef.current;
    if (!panel || activeIndex < 0) {
      return;
    }

    const activeOption = panel.querySelector<HTMLElement>(`[data-option-index="${activeIndex}"]`);
    activeOption?.scrollIntoView({ block: "nearest" });
  }

  function openMenu() {
    if (disabled || !hasAnyOption || isOpen) {
      return;
    }

    setActiveToSelected();
    setIsOpen(true);
    onOpen?.();
  }

  function toggleMenu() {
    if (isOpen) {
      closeMenu();
      return;
    }

    openMenu();
  }

  function focusTrigger() {
    window.requestAnimationFrame(() => {
      triggerRef.current?.focus();
    });
  }

  function selectOption(option: SelectOption<T>) {
    if (option.disabled) {
      return;
    }

    onChange(option.value);
    closeMenu();
    focusTrigger();
  }

  function moveActive(step: 1 | -1) {
    if (flatOptions.length === 0) {
      return;
    }

    let nextIndex = activeIndex;

    for (let index = 0; index < flatOptions.length; index += 1) {
      nextIndex = (nextIndex + step + flatOptions.length) % flatOptions.length;
      if (!flatOptions[nextIndex]?.disabled) {
        setActiveIndex(nextIndex);
        window.requestAnimationFrame(scrollActiveOptionIntoView);
        return;
      }
    }
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLButtonElement>) {
    if (disabled) {
      return;
    }

    if (!isOpen) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
        event.preventDefault();
        openMenu();
      }
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActive(1);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(-1);
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      setActiveIndex(getFirstEnabledIndex());
      window.requestAnimationFrame(scrollActiveOptionIntoView);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      const reversedIndex = [...flatOptions].reverse().findIndex((option) => !option.disabled);
      if (reversedIndex >= 0) {
        setActiveIndex(flatOptions.length - reversedIndex - 1);
        window.requestAnimationFrame(scrollActiveOptionIntoView);
      }
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const option = flatOptions[activeIndex];
      if (option) {
        selectOption(option);
      }
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      closeMenu();
      focusTrigger();
      return;
    }

    if (event.key === "Tab") {
      closeMenu();
    }
  }

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      const target = event.target as Node | null;
      if (!target) {
        return;
      }

      if (triggerRef.current?.contains(target) || panelRef.current?.contains(target)) {
        return;
      }

      closeMenu();
    }

    function handleWindowChange() {
      syncPanelPosition();
    }

    document.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("resize", handleWindowChange);
    window.addEventListener("scroll", handleWindowChange, true);
    window.requestAnimationFrame(() => {
      syncPanelPosition();
      scrollActiveOptionIntoView();
    });

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("resize", handleWindowChange);
      window.removeEventListener("scroll", handleWindowChange, true);
    };
  }, [isOpen, activeIndex, flatOptions, listMaxHeight, modelValue]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    setActiveToSelected();
    window.requestAnimationFrame(scrollActiveOptionIntoView);
  }, [isOpen, modelValue]);

  return (
    <div className={cn("w-full", className)}>
      <button
        ref={triggerRef}
        className={cn(
          "inline-flex min-h-8 w-full cursor-pointer items-center justify-between gap-2.5 rounded-sm border border-border",
          "bg-surface px-2.5 py-1.5 text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] transition-[border-color,background,box-shadow,transform]",
          "hover:border-border-hover hover:bg-surface-hover",
          "focus-visible:border-primary-soft-border focus-visible:shadow-[0_0_0_3px_var(--color-primary-soft-bg)] focus-visible:outline-none",
          isOpen && "border-primary-soft-border bg-surface-hover shadow-[0_0_0_3px_var(--color-primary-soft-bg)]",
          disabled && "cursor-not-allowed opacity-55",
          triggerClassName,
        )}
        aria-expanded={isOpen}
        aria-haspopup="listbox"
        aria-label={resolvedAriaLabel}
        type="button"
        onClick={toggleMenu}
        onKeyDown={handleKeyDown}
      >
        <span className="min-w-0 flex-1 text-left">
          {renderSelected ? renderSelected(selectedOption) : (
            <span className={cn("block text-xs leading-[1.4]", selectedOption ? "text-foreground" : "text-foreground-muted")}>
              {selectedOption?.label ?? resolvedPlaceholder}
            </span>
          )}
        </span>
        <svg
          className={cn(
            "size-3 shrink-0 text-foreground-muted transition-transform",
            isOpen && "rotate-180 text-primary-soft-text",
          )}
          viewBox="0 0 12 12"
          fill="none"
          aria-hidden="true"
        >
          <path d="M2.5 4.5L6 8L9.5 4.5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>

      {isOpen && typeof document !== "undefined" && createPortal(
        <div
          ref={panelRef}
          className={cn(
            "fixed z-[1100] overflow-y-auto rounded-md border border-border bg-surface-elevated p-1.5 shadow-overlay",
            "[backdrop-filter:blur(var(--backdrop-blur))]",
          )}
          style={panelStyle}
          role="listbox"
          aria-label={resolvedAriaLabel}
        >
          {panelEntries.map((entry, index) => {
            if (entry.kind === "group") {
              return (
                <div
                  key={`group-${index}`}
                  className="pointer-events-none px-2 pt-2 pb-1 text-[10px] leading-[1.4] font-semibold tracking-[0.04em] text-foreground-muted uppercase select-none first:pt-0.5"
                  role="presentation"
                >
                  {entry.label}
                </div>
              );
            }

            const { option, optionIndex } = entry;
            const selected = option.value === modelValue;
            const active = optionIndex === activeIndex;

            return (
              <button
                key={String(option.value)}
                className={cn(
                  "flex min-h-[34px] w-full items-center rounded-[5px] px-2 py-[7px] text-left text-foreground transition-colors",
                  (active || selected) && "bg-ghost-hover",
                  selected && "bg-primary-soft-bg text-primary-soft-text shadow-[0_0_0_1px_var(--color-primary-soft-border)]",
                  option.disabled && "cursor-not-allowed opacity-45",
                )}
                aria-selected={selected}
                data-option-index={optionIndex}
                disabled={option.disabled}
                role="option"
                tabIndex={-1}
                type="button"
                onClick={() => selectOption(option)}
                onMouseEnter={() => setActiveIndex(optionIndex)}
              >
                {renderOption ? renderOption({ option, selected, active }) : (
                  <span className="block text-xs leading-[1.4]">{option.label}</span>
                )}
              </button>
            );
          })}
        </div>,
        document.body,
      )}
    </div>
  );
}
