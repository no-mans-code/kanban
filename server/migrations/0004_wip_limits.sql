-- An optional soft cap on how many tickets a column should hold. NULL
-- means unlimited. It's advisory, not enforced: moving a ticket in past
-- the limit still works, the board just shows it's over.
ALTER TABLE statuses ADD COLUMN wip_limit INTEGER CHECK (wip_limit IS NULL OR wip_limit >= 1);
