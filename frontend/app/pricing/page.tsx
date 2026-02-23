"use client";

import { useState } from "react";
import { motion } from "framer-motion";
import { Check } from "lucide-react";

export default function PricingPage() {
    const [isYearly, setIsYearly] = useState(false);

    return (
        <section className="relative min-h-[120vh] bg-[#000000] flex flex-col items-center pt-32 pb-20 overflow-hidden font-sans w-full">

            {/* Background massive 'Pricing' text */}
            <div className="absolute top-[120px] md:top-[140px] left-1/2 -translate-x-1/2 w-full flex justify-center pointer-events-none select-none z-0">
                <h2
                    className="text-[120px] md:text-[200px] lg:text-[280px] font-bold text-white/[0.03] leading-none tracking-tighter"
                >
                    Pricing
                </h2>
            </div>

            <div className="relative z-10 w-full max-w-6xl px-4 mt-32 md:mt-40 lg:mt-48">
                <div className="grid md:grid-cols-3 gap-1 lg:gap-2 max-w-[1000px] mx-auto">

                    {/* Left Free Plan Card */}
                    <div className="group relative rounded-3xl bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 flex flex-col min-h-[460px] shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)]">
                        <div className="mb-4 text-[13px] text-[#A1A1AA] font-normal tracking-wide">Free Plan</div>
                        <div className="mb-10">
                            <span className="text-4xl lg:text-[44px] font-semibold text-white tracking-tight">Free</span>
                        </div>

                        <div className="w-full h-px bg-white/[0.06] mb-8" />

                        <ul className="flex-1 space-y-5 mb-10">
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Send up to 2 transfers per month
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Basic transaction history
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Email support
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Limited currency support (USD, EUR, GBP)
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Basic security features
                            </li>
                        </ul>

                        <button className="w-full py-[14px] rounded-full bg-transparent border border-white/[0.1] text-white text-[13px] font-medium hover:bg-white/[0.05] transition-colors">
                            Get Started
                        </button>
                    </div>

                    {/* Standard Plan Card (Center) */}
                    <div className="group relative rounded-3xl bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 flex flex-col min-h-[460px] shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)] -mt-2 mb-2 scale-[1.02] z-10">
                        <div className="mb-4 text-[13px] text-[#A1A1AA] font-normal tracking-wide">Standard Plan</div>
                        <div className="mb-10">
                            <span className="text-4xl lg:text-[44px] font-semibold text-white tracking-tight">
                                $9.99<span className="text-[28px] text-[#71717A] font-normal">/m</span>
                            </span>
                        </div>

                        <div className="w-full h-px bg-white/[0.06] mb-8" />

                        <ul className="flex-1 space-y-5 mb-10">
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Unlimited transfers
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Transaction history with export options
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Priority email support
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Expanded currency support
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Advanced security features
                            </li>
                        </ul>

                        <button className="w-full py-[14px] rounded-full bg-white text-black text-[13px] font-semibold hover:bg-neutral-200 transition-colors">
                            Get Started
                        </button>
                    </div>

                    {/* Right Free Plan Card (Matches image which says Free Plan with $19.99) */}
                    <div className="group relative rounded-3xl bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 flex flex-col min-h-[460px] shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)]">
                        <div className="mb-4 text-[13px] text-[#A1A1AA] font-normal tracking-wide">Free Plan</div>
                        <div className="mb-10">
                            <span className="text-4xl lg:text-[44px] font-semibold text-white tracking-tight">
                                $19.99<span className="text-[28px] text-[#71717A] font-normal">/m</span>
                            </span>
                        </div>

                        <div className="w-full h-px bg-white/[0.06] mb-8" />

                        <ul className="flex-1 space-y-5 mb-10">
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Unlimited transfers with priority processing
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Comprehensive transaction analytics
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                24/7 priority support
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Full currency support
                            </li>
                            <li className="flex items-start gap-4 text-[13px] text-[#A1A1AA] font-light">
                                <div className="w-5 h-5 rounded-full bg-[#18181b] flex items-center justify-center shrink-0 mt-0.5 border border-white/[0.05]">
                                    <Check size={12} className="text-[#A1A1AA]" />
                                </div>
                                Enhanced security features
                            </li>
                        </ul>

                        <button className="w-full py-[14px] rounded-full bg-transparent border border-white/[0.1] text-white text-[13px] font-medium hover:bg-white/[0.05] transition-colors">
                            Get Started
                        </button>
                    </div>

                </div>

                {/* Toggle / Billed Yearly (aligned left below cards like image) */}
                <div className="max-w-[1000px] mx-auto mt-6 flex items-center gap-4 pl-4">
                    <button
                        type="button"
                        role="switch"
                        aria-checked={isYearly}
                        onClick={() => setIsYearly(!isYearly)}
                        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-white/[0.1] transition-colors duration-200 ease-in-out focus:outline-none bg-white`}
                    >
                        <span
                            aria-hidden="true"
                            className={`pointer-events-none inline-block h-[14px] w-[14px] transform rounded-full shadow ring-0 transition duration-200 ease-in-out bg-black translate-x-1`}
                        />
                    </button>
                    <span className="text-[13px] font-medium text-[#A1A1AA] tracking-wide">Billed Yearly</span>
                </div>

            </div>
        </section>
    );
}
