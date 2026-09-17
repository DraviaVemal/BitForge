import { useEffect, useState } from "react";
import { setConfirmListener, type ConfirmRequest } from "../lib/confirm";

export default function ConfirmHost() {
  const [request, setRequest] = useState<ConfirmRequest | null>(null);

  useEffect(() => {
    setConfirmListener((next) => setRequest(next));
    return () => setConfirmListener(null);
  }, []);

  if (!request) return null;

  const close = (ok: boolean) => {
    request.resolve(ok);
    setRequest(null);
  };

  return (
    <div className="modal-overlay" onClick={() => close(false)}>
      <div className="modal confirm-modal" onClick={(event) => event.stopPropagation()}>
        <h3>{request.title ?? "Please confirm"}</h3>
        <p className="confirm-message">{request.message}</p>
        <div className="confirm-actions">
          <button className="secondary" onClick={() => close(false)}>
            {request.cancelText ?? "Cancel"}
          </button>
          <button
            className={request.danger ? "danger" : "primary"}
            onClick={() => close(true)}
            autoFocus
          >
            {request.confirmText ?? "Confirm"}
          </button>
        </div>
      </div>
    </div>
  );
}
