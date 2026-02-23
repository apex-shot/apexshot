"use client";

import { motion } from "framer-motion";

const releases = [
    {
        version: "v1.2.0",
        date: "October 24, 2026",
        badge: "Major Update",
        title: "The Performance Update",
        description: "We've entirely rebuilt our capture engine from the ground up for native Linux Wayland and X11 support, resulting in 40% faster capture speeds and zero dropped frames.",
        features: [
            "Native Wayland portal support rewritten in Rust.",
            "Hardware accelerated video encoding (NVENC & VA-API).",
            "Brand new annotation tools: Spotlight and Blur.",
            "Customizable keyboard shortcuts for every action."
        ],
        fixes: [
            "Fixed an issue where multi-monitor setups with different DPIs caused misaligned captures.",
            "Resolved a memory leak when keeping the editor open for more than 24 hours."
        ]
    },
    {
        version: "v1.1.4",
        date: "September 12, 2026",
        badge: "Improvements",
        title: "Smoother Cloud Uploads",
        description: "Small but mighty improvements to how ApexShot handles your cloud links.",
        features: [
            "Images are now instantly copied to clipboard even while uploading in the background.",
            "Added support for custom S3 buckets."
        ],
        fixes: [
            "Fixed a crash that occurred when uploading extremely large GIFs.",
            "Improved stability when switching between capturing mode and recording mode."
        ]
    },
    {
        version: "v1.1.0",
        date: "August 02, 2026",
        badge: "Feature Release",
        title: "Introducing GIF Recording",
        description: "You asked for it, and now it's here. Capture your screen as a high-quality GIF with custom framerate and sizing controls directly inside the editor.",
        features: [
            "New GIF export format with adjustable framerate (10-30fps) and dithering algorithms.",
            "Added dark-mode optimization for perfectly matching your desktop aesthetic.",
            "New floating Quick Tools palette."
        ],
        fixes: []
    },
    {
        version: "v1.0.0",
        date: "June 15, 2026",
        badge: "Initial Release",
        title: "Hello World",
        description: "The first stable release of ApexShot. A premium screen capture and annotation suite natively built for Linux enthusiasts.",
        features: [
            "High fidelity smart-selection area capture.",
            "Beautiful canvas editor with shadows, rounded corners, and backgrounds.",
            "Text, arrow, and shape annotations.",
            "Instant cloud uploads."
        ],
        fixes: []
    }
];

export default function ChangelogPage() {
    return (
        <section className="relative min-h-[120vh] bg-[#000000] flex flex-col items-center pt-32 pb-20 overflow-hidden font-sans w-full">

            {/* Background massive 'Changelog' text */}
            <div className="absolute top-[120px] md:top-[140px] left-1/2 -translate-x-1/2 w-full flex justify-center pointer-events-none select-none z-0">
                <h2
                    className="text-[100px] md:text-[180px] lg:text-[240px] font-bold text-white/[0.03] leading-none tracking-tighter"
                >
                    Changelog
                </h2>
            </div>

            <div className="relative z-10 w-full max-w-4xl px-4 mt-32 md:mt-40 lg:mt-48">

                {/* Header Subtext */}
                <div className="text-center mb-16 md:mb-24 px-4">
                    <motion.div
                        initial={{ opacity: 0, y: 20 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                    >
                        <h1 className="text-4xl lg:text-5xl font-semibold tracking-tight text-white mb-6">
                            ApexShot Updates
                        </h1>
                        <p className="text-[17px] text-[#A1A1AA] font-light leading-relaxed max-w-xl mx-auto">
                            New updates and improvements. Follow along as we obsessively refine the ultimate linux screen capture tool.
                        </p>
                    </motion.div>
                </div>

                <div className="space-y-16 lg:space-y-24">
                    {releases.map((release, index) => (
                        <motion.div
                            key={release.version}
                            initial={{ opacity: 0, y: 30 }}
                            whileInView={{ opacity: 1, y: 0 }}
                            viewport={{ once: true, margin: "-100px" }}
                            transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                            className="group relative rounded-[32px] bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 md:p-12 flex flex-col shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)]"
                        >
                            {/* Inner soft highlight */}
                            <div className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-white/[0.08] to-transparent pointer-events-none rounded-t-[32px]" />

                            <div className="flex flex-col md:flex-row md:items-start justify-between gap-6 md:gap-12 mb-8">
                                <div>
                                    <div className="flex items-center gap-3 mb-3">
                                        <span className="inline-flex items-center justify-center h-6 px-2.5 rounded-full bg-white/[0.08] border border-white/[0.05] text-[11px] font-medium text-white tracking-widest uppercase">
                                            {release.version}
                                        </span>
                                        <span className="inline-flex items-center justify-center h-6 px-2.5 rounded-full bg-[#18181b] border border-white/[0.05] text-[11px] font-medium text-[#A1A1AA] tracking-widest uppercase">
                                            {release.badge}
                                        </span>
                                    </div>
                                    <h2 className="text-2xl md:text-3xl font-semibold text-white tracking-tight">
                                        {release.title}
                                    </h2>
                                </div>

                                <div className="text-[15px] font-light text-[#71717A] mt-1 md:mt-2">
                                    {release.date}
                                </div>
                            </div>

                            <p className="text-[16px] text-[#A1A1AA] font-light leading-relaxed max-w-2xl mb-10">
                                {release.description}
                            </p>

                            <div className="space-y-10">
                                {release.features.length > 0 && (
                                    <div className="space-y-4">
                                        <h4 className="text-[13px] font-medium text-white tracking-wide uppercase">What's New</h4>
                                        <ul className="space-y-3">
                                            {release.features.map((feature, i) => (
                                                <li key={i} className="flex flex-row items-start gap-3">
                                                    <div className="w-1.5 h-1.5 rounded-full bg-white/[0.15] shrink-0 mt-[9px]" />
                                                    <span className="text-[15px] text-[#A1A1AA] font-light leading-relaxed">{feature}</span>
                                                </li>
                                            ))}
                                        </ul>
                                    </div>
                                )}

                                {release.fixes && release.fixes.length > 0 && (
                                    <div className="space-y-4">
                                        <h4 className="text-[13px] font-medium text-white tracking-wide uppercase">Bug Fixes</h4>
                                        <ul className="space-y-3">
                                            {release.fixes.map((fix, i) => (
                                                <li key={i} className="flex flex-row items-start gap-3">
                                                    <div className="w-1.5 h-1.5 rounded-full bg-white/[0.08] shrink-0 mt-[9px]" />
                                                    <span className="text-[15px] text-[#71717A] font-light leading-relaxed">{fix}</span>
                                                </li>
                                            ))}
                                        </ul>
                                    </div>
                                )}
                            </div>
                        </motion.div>
                    ))}
                </div>

                {/* Bottom spacer block to fade out nicely */}
                <div className="mt-20 flex justify-center pb-20">
                    <div className="w-2 h-2 rounded-full bg-white/[0.1] shadow-xl" />
                </div>

            </div>
        </section>
    );
}
