//! Starter plans offered on the New screen.
//!
//! A template is declared as flat rows plus a predecessor string per row, which
//! is the same shape the grid edits, so the templates double as fixtures.

use chrono::NaiveDateTime;

use crate::model::{Assignment, Project, Resource};
use crate::MINUTES_PER_DAY;

/// outline level, name, duration in days, predecessor cell text.
pub struct Row(pub u16, pub &'static str, pub f64, pub &'static str);

/// name, group, standard hourly rate.
pub struct Res(pub &'static str, pub &'static str, pub f64);

/// 1-based row number, resource name, units.
pub struct Booking(pub usize, pub &'static str, pub f64);

pub struct TemplateSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub glyph: &'static str,
    pub accent: &'static str,
    pub rows: &'static [Row],
    pub resources: &'static [Res],
    pub bookings: &'static [Booking],
}

impl TemplateSpec {
    pub fn task_count(&self) -> usize {
        self.rows.len()
    }
}

/// Turn a template into a live project. The caller schedules it afterwards.
pub fn build(spec: &TemplateSpec, start: NaiveDateTime) -> Project {
    let mut project = Project::blank(start);
    project.name = if spec.id == "blank" {
        "Project1".into()
    } else {
        spec.name.into()
    };

    for row in spec.rows {
        let minutes = (row.2 * MINUTES_PER_DAY as f64).round() as i64;
        let id = project.push_task(row.1, minutes);
        if let Some(task) = project.task_mut(id) {
            task.outline_level = row.0;
        }
    }

    // Predecessors are resolved after every row exists so forward references work.
    let ids: Vec<_> = project.tasks.iter().map(|t| t.id).collect();
    for (index, row) in spec.rows.iter().enumerate() {
        if !row.3.is_empty() {
            project.set_predecessor_text(ids[index], row.3);
        }
    }

    for res in spec.resources {
        let id = project.allocate_resource_id();
        project.resources.push(
            Resource::new(id, res.0)
                .with_group(res.1)
                .with_rate(res.2),
        );
    }

    for booking in spec.bookings {
        let Some(resource) = project
            .resources
            .iter()
            .find(|r| r.name == booking.1)
            .map(|r| r.id)
        else {
            continue;
        };
        if let Some(task) = project.tasks.get_mut(booking.0.saturating_sub(1)) {
            task.assignments.push(Assignment {
                resource,
                units: booking.2,
            });
        }
    }

    project
}

pub fn all() -> &'static [TemplateSpec] {
    TEMPLATES
}

pub fn by_id(id: &str) -> Option<&'static TemplateSpec> {
    TEMPLATES.iter().find(|t| t.id == id)
}

static TEMPLATES: &[TemplateSpec] = &[
    TemplateSpec {
        id: "blank",
        name: "Blank Project",
        description: "An empty plan. Start typing tasks straight into the grid.",
        glyph: "\u{f0fe}",
        accent: "#31752f",
        rows: &[],
        resources: &[],
        bookings: &[],
    },
    TemplateSpec {
        id: "simple",
        name: "Simple Project Plan",
        description: "A short four-phase plan with a kickoff and a closing milestone.",
        glyph: "\u{f0ae}",
        accent: "#2b579a",
        rows: &[
            Row(0, "Initiation", 0.0, ""),
            Row(1, "Kickoff meeting", 1.0, ""),
            Row(1, "Define scope", 3.0, "2"),
            Row(1, "Secure sponsorship", 2.0, "3"),
            Row(1, "Scope approved", 0.0, "4"),
            Row(0, "Planning", 0.0, ""),
            Row(1, "Build the schedule", 4.0, "5"),
            Row(1, "Assign the team", 2.0, "7"),
            Row(1, "Agree the budget", 3.0, "7"),
            Row(0, "Execution", 0.0, ""),
            Row(1, "Deliver workstream one", 10.0, "8,9"),
            Row(1, "Deliver workstream two", 8.0, "8"),
            Row(1, "Integrate and review", 4.0, "11,12"),
            Row(0, "Closure", 0.0, ""),
            Row(1, "Handover", 3.0, "13"),
            Row(1, "Lessons learned", 2.0, "15"),
            Row(1, "Project complete", 0.0, "16"),
        ],
        resources: &[
            Res("Project Manager", "Management", 85.0),
            Res("Analyst", "Delivery", 65.0),
            Res("Engineer", "Delivery", 75.0),
        ],
        bookings: &[
            Booking(2, "Project Manager", 1.0),
            Booking(3, "Analyst", 1.0),
            Booking(7, "Project Manager", 1.0),
            Booking(11, "Engineer", 1.0),
            Booking(12, "Engineer", 1.0),
            Booking(15, "Project Manager", 0.5),
        ],
    },
    TemplateSpec {
        id: "software",
        name: "Software Development Plan",
        description: "Requirements through to release, with a QA and hardening phase.",
        glyph: "\u{f121}",
        accent: "#5c2d91",
        rows: &[
            Row(0, "Discovery", 0.0, ""),
            Row(1, "Stakeholder interviews", 4.0, ""),
            Row(1, "Write requirements", 5.0, "2"),
            Row(1, "Technical spike", 3.0, "2"),
            Row(1, "Requirements signed off", 0.0, "3,4"),
            Row(0, "Design", 0.0, ""),
            Row(1, "System architecture", 6.0, "5"),
            Row(1, "Data model", 4.0, "7"),
            Row(1, "Interface design", 5.0, "5"),
            Row(1, "Design review", 2.0, "8,9"),
            Row(0, "Build", 0.0, ""),
            Row(1, "Core services", 15.0, "10"),
            Row(1, "Client application", 12.0, "10"),
            Row(1, "Integrations", 8.0, "12SS+5 days"),
            Row(1, "Feature complete", 0.0, "12,13,14"),
            Row(0, "Verification", 0.0, ""),
            Row(1, "Write test suites", 6.0, "12SS+10 days"),
            Row(1, "System testing", 8.0, "15,17"),
            Row(1, "Fix defects", 6.0, "18SS+3 days"),
            Row(1, "User acceptance testing", 5.0, "18,19"),
            Row(0, "Release", 0.0, ""),
            Row(1, "Prepare release notes", 2.0, "20"),
            Row(1, "Deploy to production", 1.0, "20,22"),
            Row(1, "Post-release monitoring", 5.0, "23"),
            Row(1, "Release complete", 0.0, "24"),
        ],
        resources: &[
            Res("Tech Lead", "Engineering", 95.0),
            Res("Backend Engineer", "Engineering", 80.0),
            Res("Frontend Engineer", "Engineering", 80.0),
            Res("QA Engineer", "Quality", 70.0),
            Res("Product Manager", "Product", 90.0),
            Res("Designer", "Product", 75.0),
        ],
        bookings: &[
            Booking(2, "Product Manager", 1.0),
            Booking(3, "Product Manager", 1.0),
            Booking(4, "Tech Lead", 1.0),
            Booking(7, "Tech Lead", 1.0),
            Booking(8, "Backend Engineer", 1.0),
            Booking(9, "Designer", 1.0),
            Booking(12, "Backend Engineer", 1.0),
            Booking(13, "Frontend Engineer", 1.0),
            Booking(14, "Backend Engineer", 0.5),
            Booking(17, "QA Engineer", 1.0),
            Booking(18, "QA Engineer", 1.0),
            Booking(19, "Backend Engineer", 1.0),
            Booking(23, "Tech Lead", 0.5),
        ],
    },
    TemplateSpec {
        id: "agile",
        name: "Agile Project Management",
        description: "Backlog, three sprints and a release, sized for a scrum team.",
        glyph: "\u{f021}",
        accent: "#c43e1c",
        rows: &[
            Row(0, "Inception", 0.0, ""),
            Row(1, "Product vision", 2.0, ""),
            Row(1, "Build the backlog", 4.0, "2"),
            Row(1, "Estimate and rank", 2.0, "3"),
            Row(1, "Sprint zero complete", 0.0, "4"),
            Row(0, "Sprint 1", 0.0, ""),
            Row(1, "Sprint 1 planning", 1.0, "5"),
            Row(1, "Sprint 1 development", 8.0, "7"),
            Row(1, "Sprint 1 review", 1.0, "8"),
            Row(1, "Sprint 1 retrospective", 0.5, "9"),
            Row(0, "Sprint 2", 0.0, ""),
            Row(1, "Sprint 2 planning", 1.0, "10"),
            Row(1, "Sprint 2 development", 8.0, "12"),
            Row(1, "Sprint 2 review", 1.0, "13"),
            Row(1, "Sprint 2 retrospective", 0.5, "14"),
            Row(0, "Sprint 3", 0.0, ""),
            Row(1, "Sprint 3 planning", 1.0, "15"),
            Row(1, "Sprint 3 development", 8.0, "17"),
            Row(1, "Sprint 3 review", 1.0, "18"),
            Row(1, "Sprint 3 retrospective", 0.5, "19"),
            Row(0, "Release", 0.0, ""),
            Row(1, "Hardening", 4.0, "20"),
            Row(1, "Release to production", 1.0, "22"),
            Row(1, "Increment shipped", 0.0, "23"),
        ],
        resources: &[
            Res("Scrum Master", "Team", 85.0),
            Res("Product Owner", "Team", 90.0),
            Res("Developer", "Team", 80.0),
            Res("Tester", "Team", 70.0),
        ],
        bookings: &[
            Booking(2, "Product Owner", 1.0),
            Booking(3, "Product Owner", 1.0),
            Booking(8, "Developer", 1.0),
            Booking(13, "Developer", 1.0),
            Booking(18, "Developer", 1.0),
            Booking(22, "Tester", 1.0),
        ],
    },
    TemplateSpec {
        id: "construction",
        name: "Residential Construction",
        description: "Ground works to handover for a single dwelling, with inspections.",
        glyph: "\u{f1ad}",
        accent: "#986f0b",
        rows: &[
            Row(0, "Pre-construction", 0.0, ""),
            Row(1, "Site survey", 3.0, ""),
            Row(1, "Planning permission", 20.0, "2"),
            Row(1, "Tender and award", 10.0, "3"),
            Row(1, "Permit granted", 0.0, "3"),
            Row(0, "Substructure", 0.0, ""),
            Row(1, "Site clearance", 4.0, "4,5"),
            Row(1, "Excavation", 5.0, "7"),
            Row(1, "Foundations", 8.0, "8"),
            Row(1, "Foundation inspection", 1.0, "9"),
            Row(0, "Superstructure", 0.0, ""),
            Row(1, "Ground floor slab", 5.0, "10"),
            Row(1, "External walls", 15.0, "12"),
            Row(1, "Roof structure", 8.0, "13"),
            Row(1, "Roof covering", 6.0, "14"),
            Row(1, "Watertight", 0.0, "15"),
            Row(0, "First fix", 0.0, ""),
            Row(1, "Electrical first fix", 6.0, "16"),
            Row(1, "Plumbing first fix", 6.0, "16"),
            Row(1, "Insulation", 4.0, "18,19"),
            Row(1, "Plasterboard", 8.0, "20"),
            Row(0, "Second fix", 0.0, ""),
            Row(1, "Plastering", 7.0, "21"),
            Row(1, "Electrical second fix", 5.0, "23"),
            Row(1, "Plumbing second fix", 5.0, "23"),
            Row(1, "Joinery", 8.0, "23"),
            Row(1, "Decoration", 8.0, "24,25,26"),
            Row(0, "Completion", 0.0, ""),
            Row(1, "External works", 10.0, "16"),
            Row(1, "Final inspection", 2.0, "27,29"),
            Row(1, "Snagging", 5.0, "30"),
            Row(1, "Handover", 0.0, "31"),
        ],
        resources: &[
            Res("Site Manager", "Management", 70.0),
            Res("Groundworker", "Trades", 45.0),
            Res("Bricklayer", "Trades", 50.0),
            Res("Carpenter", "Trades", 52.0),
            Res("Electrician", "Trades", 58.0),
            Res("Plumber", "Trades", 58.0),
            Res("Plasterer", "Trades", 48.0),
            Res("Decorator", "Trades", 42.0),
        ],
        bookings: &[
            Booking(7, "Groundworker", 1.0),
            Booking(8, "Groundworker", 1.0),
            Booking(9, "Groundworker", 1.0),
            Booking(13, "Bricklayer", 1.0),
            Booking(14, "Carpenter", 1.0),
            Booking(18, "Electrician", 1.0),
            Booking(19, "Plumber", 1.0),
            Booking(23, "Plasterer", 1.0),
            Booking(24, "Electrician", 1.0),
            Booking(25, "Plumber", 1.0),
            Booking(26, "Carpenter", 1.0),
            Booking(27, "Decorator", 1.0),
        ],
    },
    TemplateSpec {
        id: "marketing",
        name: "Marketing Campaign Plan",
        description: "Strategy, creative production, launch and post-campaign analysis.",
        glyph: "\u{f0a1}",
        accent: "#b4009e",
        rows: &[
            Row(0, "Strategy", 0.0, ""),
            Row(1, "Market research", 6.0, ""),
            Row(1, "Define audience", 3.0, "2"),
            Row(1, "Set campaign goals", 2.0, "3"),
            Row(1, "Budget approval", 4.0, "4"),
            Row(0, "Creative", 0.0, ""),
            Row(1, "Creative brief", 3.0, "5"),
            Row(1, "Concept development", 6.0, "7"),
            Row(1, "Copywriting", 5.0, "8"),
            Row(1, "Design assets", 8.0, "8"),
            Row(1, "Video production", 10.0, "8"),
            Row(1, "Creative sign-off", 2.0, "9,10,11"),
            Row(0, "Channels", 0.0, ""),
            Row(1, "Media plan", 4.0, "5"),
            Row(1, "Book placements", 5.0, "14"),
            Row(1, "Landing page build", 7.0, "12"),
            Row(1, "Email sequence setup", 4.0, "12"),
            Row(0, "Launch", 0.0, ""),
            Row(1, "Soft launch", 2.0, "15,16,17"),
            Row(1, "Campaign live", 0.0, "19"),
            Row(1, "In-flight optimisation", 15.0, "20"),
            Row(0, "Wrap up", 0.0, ""),
            Row(1, "Performance analysis", 5.0, "21"),
            Row(1, "Report to stakeholders", 3.0, "23"),
        ],
        resources: &[
            Res("Marketing Lead", "Marketing", 80.0),
            Res("Copywriter", "Creative", 60.0),
            Res("Designer", "Creative", 65.0),
            Res("Video Producer", "Creative", 75.0),
            Res("Media Buyer", "Marketing", 70.0),
            Res("Web Developer", "Digital", 78.0),
        ],
        bookings: &[
            Booking(2, "Marketing Lead", 1.0),
            Booking(9, "Copywriter", 1.0),
            Booking(10, "Designer", 1.0),
            Booking(11, "Video Producer", 1.0),
            Booking(14, "Media Buyer", 1.0),
            Booking(16, "Web Developer", 1.0),
            Booking(23, "Marketing Lead", 0.5),
        ],
    },
    TemplateSpec {
        id: "launch",
        name: "New Product Launch",
        description: "Readiness across product, supply, sales and support up to launch day.",
        glyph: "\u{f135}",
        accent: "#038387",
        rows: &[
            Row(0, "Readiness", 0.0, ""),
            Row(1, "Confirm launch date", 1.0, ""),
            Row(1, "Pricing approved", 5.0, "2"),
            Row(1, "Positioning finalised", 5.0, "2"),
            Row(0, "Product", 0.0, ""),
            Row(1, "Release candidate", 10.0, "3,4"),
            Row(1, "Beta programme", 12.0, "6"),
            Row(1, "Launch build signed off", 0.0, "7"),
            Row(0, "Supply", 0.0, ""),
            Row(1, "Forecast demand", 4.0, "3"),
            Row(1, "Place manufacturing order", 3.0, "10"),
            Row(1, "Production run", 20.0, "11"),
            Row(1, "Stock in warehouse", 5.0, "12"),
            Row(0, "Go to market", 0.0, ""),
            Row(1, "Sales enablement", 6.0, "4"),
            Row(1, "Train support team", 5.0, "8"),
            Row(1, "Press and analyst briefing", 4.0, "4"),
            Row(1, "Launch assets ready", 8.0, "4"),
            Row(0, "Launch", 0.0, ""),
            Row(1, "Launch readiness review", 2.0, "8,13,15,16,17,18"),
            Row(1, "Launch day", 0.0, "20"),
            Row(1, "First week review", 5.0, "21"),
        ],
        resources: &[
            Res("Product Manager", "Product", 90.0),
            Res("Engineering Lead", "Product", 95.0),
            Res("Supply Planner", "Operations", 68.0),
            Res("Sales Enablement", "Commercial", 72.0),
            Res("PR Manager", "Marketing", 76.0),
        ],
        bookings: &[
            Booking(3, "Product Manager", 1.0),
            Booking(6, "Engineering Lead", 1.0),
            Booking(10, "Supply Planner", 1.0),
            Booking(12, "Supply Planner", 0.5),
            Booking(15, "Sales Enablement", 1.0),
            Booking(17, "PR Manager", 1.0),
        ],
    },
    TemplateSpec {
        id: "event",
        name: "Event Planning",
        description: "Venue, programme, promotion and run of show for a one-day conference.",
        glyph: "\u{f073}",
        accent: "#8764b8",
        rows: &[
            Row(0, "Foundations", 0.0, ""),
            Row(1, "Agree objectives", 2.0, ""),
            Row(1, "Set the budget", 3.0, "2"),
            Row(1, "Choose the date", 1.0, "2"),
            Row(0, "Venue and logistics", 0.0, ""),
            Row(1, "Shortlist venues", 5.0, "4"),
            Row(1, "Site visits", 4.0, "6"),
            Row(1, "Contract the venue", 3.0, "7"),
            Row(1, "Catering booked", 4.0, "8"),
            Row(1, "AV and staging booked", 5.0, "8"),
            Row(0, "Programme", 0.0, ""),
            Row(1, "Call for speakers", 10.0, "4"),
            Row(1, "Select speakers", 5.0, "12"),
            Row(1, "Build the agenda", 4.0, "13"),
            Row(1, "Brief the speakers", 5.0, "14"),
            Row(0, "Promotion", 0.0, ""),
            Row(1, "Event website live", 6.0, "14"),
            Row(1, "Open registration", 1.0, "17"),
            Row(1, "Promotion campaign", 25.0, "18"),
            Row(1, "Registration closes", 0.0, "19"),
            Row(0, "Delivery", 0.0, ""),
            Row(1, "Final headcount to venue", 2.0, "20"),
            Row(1, "Print badges and signage", 3.0, "20"),
            Row(1, "Set up", 1.0, "9,10,22,23"),
            Row(1, "Event day", 1.0, "24"),
            Row(1, "Tear down", 1.0, "25"),
            Row(1, "Attendee survey", 5.0, "25"),
        ],
        resources: &[
            Res("Event Manager", "Events", 75.0),
            Res("Logistics Coordinator", "Events", 55.0),
            Res("Marketing Lead", "Marketing", 80.0),
            Res("AV Technician", "Suppliers", 60.0),
        ],
        bookings: &[
            Booking(6, "Event Manager", 1.0),
            Booking(9, "Logistics Coordinator", 1.0),
            Booking(10, "AV Technician", 1.0),
            Booking(19, "Marketing Lead", 1.0),
            Booking(24, "Logistics Coordinator", 1.0),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::schedule;
    use chrono::NaiveDate;

    #[test]
    fn every_template_builds_and_schedules() {
        let start = NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();

        for spec in all() {
            let mut project = build(spec, start);
            assert_eq!(project.tasks.len(), spec.rows.len(), "{}", spec.id);
            let report = schedule(&mut project)
                .unwrap_or_else(|e| panic!("template {} failed to schedule: {e}", spec.id));
            if !spec.rows.is_empty() {
                assert!(report.finish >= report.start, "{}", spec.id);
                assert!(
                    report.critical_task_count > 0,
                    "{} should have a critical path",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn template_predecessors_all_resolve() {
        let start = NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();

        for spec in all() {
            let project = build(spec, start);
            let expected: usize = spec
                .rows
                .iter()
                .map(|r| r.3.split(',').filter(|s| !s.trim().is_empty()).count())
                .sum();
            assert_eq!(
                project.links.len(),
                expected,
                "template {} dropped a predecessor reference",
                spec.id
            );
        }
    }
}
