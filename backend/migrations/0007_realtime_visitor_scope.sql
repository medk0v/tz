ALTER TABLE realtime_tickets
    ADD COLUMN can_read_visitor_network boolean NOT NULL DEFAULT false;
