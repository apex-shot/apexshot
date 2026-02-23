"use client";

import Link from "next/link";
import { Github, Twitter, Mail, ArrowRight, ArrowUp } from "lucide-react";

export function Footer() {
    const scrollToTop = () => {
        if (typeof window !== "undefined") {
            window.scrollTo({ top: 0, behavior: 'smooth' });
        }
    };

    return (
        <section className="bg-[#000000] pt-40 w-full font-sans border-t border-white/[0.05]">
            <div className="w-[90%] md:w-[80%] mx-auto px-6">
                {/* Floating Call to Action */}
                <div className="relative z-10 -mb-24">
                    <div className="bg-[#09090b] rounded-[32px] overflow-hidden h-96 relative group shadow-[0_40px_80px_-20px_rgba(0,0,0,1)] border border-white/[0.05]">
                        <img
                            src="https://images.unsplash.com/photo-1549979047-f06bb9619b61?q=80&w=1374&auto=format&fit=crop"
                            alt="Background"
                            className="w-full h-full object-cover opacity-40 mix-blend-luminosity brightness-75 transition-transform duration-1000 group-hover:scale-105"
                        />
                        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-black/20 flex flex-col justify-center p-10 md:p-20 z-10">
                            <h2 className="text-white text-4xl md:text-5xl lg:text-6xl font-semibold max-w-2xl mb-8 tracking-tighter leading-[1.1]">
                                Let's build your ideal workflow today.
                            </h2>
                            <Link href="/pricing" className="bg-[#F4F4F5] text-black px-8 py-4 rounded-full w-fit flex items-center gap-3 font-semibold hover:bg-white transition-colors group/btn">
                                Start capturing
                                <div className="size-8 bg-black rounded-full flex items-center justify-center text-white">
                                    <ArrowRight className="size-4 group-hover/btn:translate-x-0.5 transition-transform" />
                                </div>
                            </Link>
                        </div>
                    </div>
                </div>
            </div>

            <div className="w-full">
                {/* Main Dark Footer Area */}
                <div className="bg-[#050505] rounded-t-[40px] border-t border-white/[0.05] pt-40 pb-12 px-6 md:px-12 text-white">
                    <div className="max-w-7xl mx-auto">
                        <div className="grid grid-cols-1 lg:grid-cols-2 gap-12 items-start pb-16">

                            <div className="space-y-12">
                                {/* Branding */}
                                <div className="flex items-center gap-4">
                                    <div className="relative flex h-[48px] w-[48px] items-center justify-center rounded-[14px] bg-gradient-to-b from-[#18181b] to-[#09090b] shadow-[inset_0_1px_rgba(255,255,255,0.1),_0_4px_12px_rgba(0,0,0,0.8)] border border-white/[0.05]">
                                        <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="url(#apex-gradient-footer)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="relative z-10">
                                            <defs>
                                                <linearGradient id="apex-gradient-footer" x1="0%" y1="0%" x2="100%" y2="100%">
                                                    <stop offset="0%" stopColor="#ffffff" />
                                                    <stop offset="100%" stopColor="#a1a1aa" />
                                                </linearGradient>
                                            </defs>
                                            <path d="M4 22L12 4L20 22" />
                                            <path d="M2 15H22" strokeDasharray="3 4" strokeWidth="1.5" />
                                            <circle cx="12" cy="4" r="1.5" fill="#ffffff" stroke="none" />
                                        </svg>
                                    </div>
                                    <div className="text-4xl font-semibold tracking-[-0.04em] text-[#F4F4F5]">
                                        Apex<span className="text-[#A1A1AA] font-light">s h o t</span>
                                    </div>
                                </div>

                                <div className="space-y-4">
                                    <h4 className="text-[13px] font-medium text-[#71717A] uppercase tracking-widest">
                                        Connect
                                    </h4>
                                    <div className="flex gap-4">
                                        <a href="https://github.com" className="size-10 border border-white/[0.1] rounded-full flex items-center justify-center text-[#A1A1AA] hover:bg-white hover:text-black transition-colors">
                                            <Github className="size-4" />
                                        </a>
                                        <a href="https://twitter.com" className="size-10 border border-white/[0.1] rounded-full flex items-center justify-center text-[#A1A1AA] hover:bg-white hover:text-black transition-colors">
                                            <Twitter className="size-4" />
                                        </a>
                                        <a href="mailto:hello@apexshot.com" className="size-10 border border-white/[0.1] rounded-full flex items-center justify-center text-[#A1A1AA] hover:bg-white hover:text-black transition-colors">
                                            <Mail className="size-4" />
                                        </a>
                                    </div>
                                </div>
                            </div>

                            <div className="space-y-6 lg:text-right">
                                <h3 className="text-lg font-medium text-[#F4F4F5]">
                                    Subscribe for ApexShot updates & insights
                                </h3>
                                <p className="text-[15px] text-[#A1A1AA] max-w-sm lg:ml-auto leading-relaxed">
                                    Get the latest release notes, productivity hacks, and exclusive early access to beta features.
                                </p>
                                <div className="relative max-w-sm lg:ml-auto flex">
                                    <input
                                        type="email"
                                        placeholder="Enter your email"
                                        className="w-full bg-[#09090b] border border-white/[0.1] rounded-full px-6 py-3.5 pr-14 text-white text-[15px] focus:outline-none focus:border-white/[0.3] transition-colors placeholder:text-[#71717A]"
                                    />
                                    <button className="absolute right-1.5 top-1/2 -translate-y-1/2 size-10 bg-white rounded-full flex items-center justify-center text-black hover:bg-neutral-200 transition-colors">
                                        <ArrowRight className="size-4" />
                                    </button>
                                </div>
                            </div>
                        </div>

                        <nav className="border-t border-white/[0.05] py-10 grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-8 text-[15px] font-medium text-[#A1A1AA]">
                            <a href="#features" className="hover:text-white transition-colors">Features</a>
                            <a href="/pricing" className="hover:text-white transition-colors">Pricing</a>
                            <a href="#docs" className="hover:text-white transition-colors">Documentation</a>
                            <Link href="/changelog" className="hover:text-white transition-colors">Changelog</Link>
                            <Link href="/about" className="hover:text-white transition-colors">About Us</Link>
                            <Link href="/contact" className="hover:text-white transition-colors">Contact</Link>
                        </nav>

                        <div className="py-6 border-t border-white/[0.05] flex flex-col md:flex-row justify-between items-center gap-6">
                            <span className="font-semibold text-white tracking-tight">ApexShot</span>
                            <span className="text-[#71717A] text-[13px] font-light">
                                © 2026 ApexShot. All rights reserved.
                            </span>
                            <button onClick={scrollToTop} className="flex items-center bg-transparent gap-3 text-[13px] font-medium text-[#A1A1AA] hover:text-white transition-colors group">
                                Back to top
                                <div className="size-8 bg-white/[0.05] border border-white/[0.1] text-white rounded-full flex items-center justify-center group-hover:bg-white group-hover:text-black transition-colors">
                                    <ArrowUp className="size-4" />
                                </div>
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        </section>
    );
}
