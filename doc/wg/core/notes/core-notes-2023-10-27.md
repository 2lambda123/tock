Tock Core Notes 2023-10-27
==========================

## Attendees
 - Pat Pannuto
 - Andrew Imwalle
 - Branden Ghena
 - Amit Levy
 - Alyssa Haroldsen
 - Brad Campbell
 - Leon Schuermann
 - Johnathan Van Why
 - Tyler Potyondy
 - Alexandru Radovici

## Welcome Andrew
 - Andrew Imwalle: Working on team at HP Enterprise on a security processor
 - Amit/Andrew: Project has been going on long enough and is now mature enough that there is likely a stream of upstreaming PRs to come (TickV zeroizing being the first)

## Networking WG Update
 - Branden: WG is bi-weekly, but as a refresh of last week's update:
 - Branden: Reserving port numbers for Thread (led to longer core team call... see last week's notes)
 - Branden: Other big focus point is buffer management—sync/async, contiguous or not, etc; Leon driving questions around how much we can make the type system to while still providing a lot of runtime usability.
 - Branden: Pretty close to something we can actually try in real interfaces. SD Card and Display drivers may actually be first target as the code paths are more mature but share similar buffer concerns.

## OT WG Update
 - Brad: Merged updated to autogenerated register interfaces; new tool removed the `top_` hardware artifact that had been leaking through
 - Brad: Also some updates coming down the pipe in GPIO and I2C
 - Amit: Historically, there had been two paths for OT+Tock, one driven by Alistair & co, and the other by OT core / etc, and we had seen some divergence. Is that still the case, or are the more recent PRs resolving some of this?
 - Johnathan: The OT repository is pulling in Tock as a dependency. We're doing what we can in the Tock project to have things directly in upstream. We will always have some indirection, e.g. custom board, but a lot of the drivers/etc will stay upstream
 - Johnathan: Long-term, OT has some minimum code standards, and it is in our interest/goal for upstream to meet internal requirements
 - Amit: What's the resolution of the OT PMP / ePMP / etc situation?
 - Leon: This has been involuntarily on the backburner for a few weeks, but it's on critical path of a research project, so things should move forward again soon
 - Leon: I've been at SOSP and the Workshop on Kernel Safety and Isolation; interesting talk on Veris, formal verification in/on Rust, from some MSR folks
 - Leon: Led to conversations around our (e)PMP implementation, and think there may be some opportunity for FV to be applied to embedded systems without being unreasonable / intractable; think (e)PMP could be an ideal subsystem to start with that
 - Leon: However, do want to get current code finished/shipped before looking to implement FV

## General Updates
 - Alexandru: Sent email to ESP, we'll see what comes

## Form a Community Working Group?
 - Amit: Proposal—we should have one
 - Amit: Primary motivation is making sure we get moving soon on TockWorld7, but imagine more in the future
 - Brad: What would be in the purview of this WG?
 - Amit: Events like TockWorld, but also other kinds of community outreach, e.g. tutorials, and possibly certain kinds of documentation
 - Pat: Maybe also the public face of Tock, the main website, the blog, etc
 - Leon: Maybe separate issues? I would be like to be interested in helping events, company outreach, etc
 - Amit: Alternative proposal is not a community WG, but a 'TockWorld Task Force'
 - Amit: The difference is that it's time-limited and more focused
 - Amit: Can take on work and discussions that we wouldn't necessarily have here, but not a WG with unbounded time
 - Leon: I think that make sense, and I would like to spend some time on that
 - Leon: A more general community WG for documentation / etc
 - Branden: Logistic concern—do we have the person-hours to staff another working group (e.g., Leon is on... all of them)
 - Amit: I agree with both of those... with my advisor hat on, I appreciate Leon's desire to help with community things, but "you got research to do man"
 - Amit: I'd propose this includes at minimum Pat (as it's going to be in San Diego)
 - Amit: I am actually not formally on any other WGs, so I volunteer; probably don't need more than 1-2 people
 - Amit: It could just be in principle me and Pat...
 - Amit: The point is largely around accountability
 - Pat: Agree that smaller-set accountability for things that 'core' is responsible for e.g. web updates useful
 - Pat: See value in continuity of event organizers; WG can persist across TockWorld events
 - Brad: Unclear if this is for 'events in 2024' or 'events generally' etc; Task Force model makes more sense now, and if TF is successful, can shed light on purview for a more sustained WG
 - Amit: Propose a TockWorld 2024 Task Force with founding members of Pat and me
 - Brad: I volunteer to be on this as well.

## Yield Wait For RFC?
 - Amit: This has stagnated for ~a month
 - Amit: I don't recall where it stands... Pat?
 - Pat: Life and family in the way, more so especially in November :/
 - Brad: I thought that we were converging; next step was to actually implement enough apps in userspace to see if it actually made a difference
 - Pat: I did some of that—was given 4 apps to port, I did console, the other three were a lot of work for speculative update and console so worth it that thought we'd decided that was sufficient
 - Amit: Okay for this to move a slow in the short-term future, but can't fall on the floor completely
 - Amit: I will, at least tentatively, take over this PR for the near-term
 - Amit: Primary mandate will be to summarize status, disposition, and next steps that need to happen; will possibly take on the implementation depending on result here
 - Alyssa: Anything you need from me right now?
 - Amit: Almost certainly yes, but I don't know what that is right now. Part of my pending summary will include specific asks where appropriate.
 - Alyssa: Next week I can have this read that we can discuss a bit better

## Open Discussion
 - Andrew: Is there any kind of formal release schedule or plan?
 - Amit: We've experimented with a few different styles. We tried a time-based cadence, but didn't fit flow well. Current model is largely critical mass of accumulated changes
 - Amit: Looking forward, we are in the process of hiring a staff engineer to help support things like CI, security audits, etc; would include managing releases, so next release could be a great thing to have them on board for
 - Andrew: Is there a timeline to hiring?
 - Amit: Have several applications in, need to conduct interview process, then hiring logistics; ideally on the order of month to hire
 - Amit: Is having a release in the (more?) near future useful?
 - Andrew: Not necessarily in the near future, but as we start contributing more, would be useful to see changes we contribute come out in stable releases
 - Branden: For a long time, we've been pushed by demand—if there's a reason a release would be useful for someone, that's a reason to do a release

Fin.
