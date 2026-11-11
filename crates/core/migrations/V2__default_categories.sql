-- The default categories (ROADMAP M1.14) and each category's kind.
-- `docs/ARCHITECTURE.md` → "Categories" lists the set and the decisions
-- behind it; `DEFAULT_CATEGORIES` in `crates/core/src/category/defaults.rs`
-- is the same list in Rust, and a test keeps the two equal.
--
-- Seeded here, not from Rust at every open, so it happens exactly once and in
-- the same transaction as the rest of the schema: a default the user later
-- deletes never comes back, and `Database::open_existing` (`coffer mcp`)
-- never writes. A later Coffer that adds defaults does so in a new
-- migration, inserting each key only where it is free, so a category the
-- user already made with that key is kept as it is.
--
-- Labels are English (en-GB, the first region); keys name the concept, not a
-- country's word for it (`home.property_tax`, labelled "Council tax").

-- What a category's amounts count as. `spending` and `income` are the two
-- sides of every report; `transfer` is money moving between the user's own
-- accounts (or to one Coffer doesn't see), which counts as neither.
ALTER TABLE category ADD COLUMN kind TEXT NOT NULL DEFAULT 'spending'
    CHECK (kind IN ('spending', 'income', 'transfer'));

-- A subcategory has its group's kind, so a group's total never mixes income
-- and spending.
CREATE TRIGGER category_kind_insert
BEFORE INSERT ON category
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a subcategory has its parent''s kind')
    FROM category WHERE id = NEW.parent_id AND kind IS NOT NEW.kind;
END;

-- A kind is fixed: changing it would silently move a category's whole history
-- between income, spending and transfers. To change it, make a new category
-- and move the rows (an undoable change).
CREATE TRIGGER category_kind_read_only
BEFORE UPDATE ON category
WHEN NEW.kind IS NOT OLD.kind
BEGIN
    SELECT RAISE(ABORT, 'a category''s kind is read-only');
END;

-- Groups, in display order.
INSERT INTO category (key, label, kind) VALUES
    ('income',        'Income',        'income'),
    ('home',          'Home',          'spending'),
    ('bills',         'Bills',         'spending'),
    ('food',          'Food',          'spending'),
    ('transport',     'Transport',     'spending'),
    ('shopping',      'Shopping',      'spending'),
    ('entertainment', 'Entertainment', 'spending'),
    ('health',        'Health',        'spending'),
    ('travel',        'Travel',        'spending'),
    ('family',        'Family',        'spending'),
    ('giving',        'Giving',        'spending'),
    ('finance',       'Finance',       'spending'),
    ('cash',          'Cash',          'spending'),
    ('other',         'Other',         'spending'),
    ('transfer',      'Transfers',     'transfer');

-- Subcategories, under their group and with its kind.
INSERT INTO category (parent_id, key, label, kind)
SELECT parent.id, child.key, child.label, parent.kind
FROM (
    SELECT 1 AS position, 'income.salary' AS key, 'Salary' AS label
    UNION ALL SELECT  2, 'income.self_employment',  'Self-employment'
    UNION ALL SELECT  3, 'income.benefits',         'Benefits'
    UNION ALL SELECT  4, 'income.pension',          'Pension'
    UNION ALL SELECT  5, 'income.interest',         'Interest'
    UNION ALL SELECT  6, 'income.investments',      'Investment income'
    UNION ALL SELECT  7, 'income.rental',           'Rental income'
    UNION ALL SELECT  8, 'income.gifts',            'Gifts received'
    UNION ALL SELECT  9, 'home.rent',               'Rent'
    UNION ALL SELECT 10, 'home.mortgage',           'Mortgage'
    UNION ALL SELECT 11, 'home.property_tax',       'Council tax'
    UNION ALL SELECT 12, 'home.maintenance',        'Repairs and maintenance'
    UNION ALL SELECT 13, 'home.household',          'Household'
    UNION ALL SELECT 14, 'bills.energy',            'Energy'
    UNION ALL SELECT 15, 'bills.water',             'Water'
    UNION ALL SELECT 16, 'bills.phone',             'Phone'
    UNION ALL SELECT 17, 'bills.internet',          'Internet'
    UNION ALL SELECT 18, 'bills.tv',                'TV'
    UNION ALL SELECT 19, 'bills.insurance',         'Insurance'
    UNION ALL SELECT 20, 'bills.subscriptions',     'Subscriptions'
    UNION ALL SELECT 21, 'food.groceries',          'Groceries'
    UNION ALL SELECT 22, 'food.eating_out',         'Eating out'
    UNION ALL SELECT 23, 'food.takeaway',           'Takeaways'
    UNION ALL SELECT 24, 'transport.public',        'Public transport'
    UNION ALL SELECT 25, 'transport.taxi',          'Taxis'
    UNION ALL SELECT 26, 'transport.fuel',          'Fuel'
    UNION ALL SELECT 27, 'transport.parking',       'Parking and tolls'
    UNION ALL SELECT 28, 'transport.vehicle',       'Vehicle costs'
    UNION ALL SELECT 29, 'shopping.clothing',       'Clothing'
    UNION ALL SELECT 30, 'shopping.electronics',    'Electronics'
    UNION ALL SELECT 31, 'entertainment.events',    'Events and tickets'
    UNION ALL SELECT 32, 'entertainment.hobbies',   'Hobbies'
    UNION ALL SELECT 33, 'health.medical',          'Medical'
    UNION ALL SELECT 34, 'health.fitness',          'Fitness'
    UNION ALL SELECT 35, 'health.personal_care',    'Personal care'
    UNION ALL SELECT 36, 'travel.flights',          'Flights'
    UNION ALL SELECT 37, 'travel.accommodation',    'Accommodation'
    UNION ALL SELECT 38, 'family.childcare',        'Childcare'
    UNION ALL SELECT 39, 'family.children',         'Children'
    UNION ALL SELECT 40, 'family.education',        'Education'
    UNION ALL SELECT 41, 'family.pets',             'Pets'
    UNION ALL SELECT 42, 'giving.gifts',            'Gifts'
    UNION ALL SELECT 43, 'giving.charity',          'Charity'
    UNION ALL SELECT 44, 'finance.fees',            'Fees and charges'
    UNION ALL SELECT 45, 'finance.interest',        'Interest paid'
    UNION ALL SELECT 46, 'finance.loans',           'Loan repayments'
    UNION ALL SELECT 47, 'finance.tax',             'Tax'
    UNION ALL SELECT 48, 'transfer.savings',        'Savings'
    UNION ALL SELECT 49, 'transfer.investments',    'Investments'
    UNION ALL SELECT 50, 'transfer.credit_card',    'Credit card payments'
) AS child
JOIN category AS parent
  ON parent.parent_id IS NULL
 AND parent.key = substr(child.key, 1, instr(child.key, '.') - 1)
ORDER BY child.position;
