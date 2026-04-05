import { users, CURRENT_USER_ID } from "../data/seed";

interface ReadReceiptsProps {
  /** seq number of this message */
  seq: number;
  /** cursor positions: user_id -> last read seq */
  cursors: Record<string, number>;
  /** stream members */
  memberIds: string[];
}

export function ReadReceipts({ seq, cursors, memberIds }: ReadReceiptsProps) {
  // Find members (excluding current user and sender) who have read up to this seq
  const readers = memberIds.filter(
    (uid) => uid !== CURRENT_USER_ID && (cursors[uid] ?? 0) >= seq,
  );

  if (readers.length === 0) return null;

  return (
    <div className="read-receipts">
      {readers.map((uid) => (
        <span key={uid} className="read-receipt-avatar" title={`Read by ${users[uid]?.name ?? uid}`}>
          {users[uid]?.avatar ?? uid[0].toUpperCase()}
        </span>
      ))}
    </div>
  );
}
