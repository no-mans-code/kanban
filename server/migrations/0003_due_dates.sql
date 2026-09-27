-- An optional due date per ticket (epoch milliseconds, like every other
-- timestamp in this schema). NULL means no due date.
ALTER TABLE tickets ADD COLUMN due_date INTEGER;
CREATE INDEX tickets_due ON tickets (due_date);
