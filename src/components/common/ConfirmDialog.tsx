import { useI18n } from "../../i18n";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

type ConfirmDialogProps = {
  open: boolean;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: "default" | "danger";
  busy?: boolean;
  ariaLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
};

export default function ConfirmDialog({
  open,
  message,
  confirmLabel,
  cancelLabel,
  variant = "default",
  busy = false,
  ariaLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useI18n();

  const resolvedConfirmLabel = confirmLabel ?? t("common.confirm");
  const resolvedCancelLabel = cancelLabel ?? t("common.cancel");
  const resolvedAriaLabel = ariaLabel ?? t("common.confirm");

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        // busy（如删除中）时忽略 Esc / 遮罩点击触发的关闭
        if (!nextOpen && !busy) {
          onCancel();
        }
      }}
    >
      <DialogContent
        className="w-full gap-3.5 rounded-xl border-border bg-surface p-4 shadow-drag sm:max-w-[360px]"
        showCloseButton={false}
        aria-label={resolvedAriaLabel}
      >
        {/* Radix 要求存在 Title 以满足无障碍，这里视觉上隐藏 */}
        <DialogTitle className="sr-only">{resolvedAriaLabel}</DialogTitle>
        <DialogDescription className="text-xs leading-[1.6] whitespace-pre-wrap break-words text-foreground">
          {message}
        </DialogDescription>
        <DialogFooter className="flex-row flex-wrap justify-end gap-2 sm:justify-end">
          <Button
            variant="softGhost"
            size="xs"
            className="min-w-[76px]"
            disabled={busy}
            type="button"
            onClick={onCancel}
          >
            {resolvedCancelLabel}
          </Button>
          <Button
            variant={variant === "danger" ? "softDanger" : "soft"}
            size="xs"
            className="min-w-[76px]"
            disabled={busy}
            type="button"
            onClick={onConfirm}
          >
            {resolvedConfirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
