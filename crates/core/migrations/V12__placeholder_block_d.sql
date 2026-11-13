-- PLACEHOLDER, Block E's branch only (M2 wave 1). Delete this file at the
-- Wave 2 merge: Block D's `V12__…` takes version 12.
--
-- Migrations must be numbered without gaps
-- (`db::migrations::tests::embedded_migrations_are_numbered_without_gaps`),
-- and Block E owns V13, so this branch needs a V12 to build and test. It
-- changes nothing. If both this file and D's V12 are present, the gap test
-- fails loudly, so it can't be merged by accident.
SELECT 1;
