import { useEffect, useEffectEvent } from "react";
import type { DependencyList } from "react";

export const useSubscription = (subscribe: () => () => void, deps: DependencyList) => {
  useEffect(subscribe, deps);
};

export const useImperative = (apply: () => void, deps: DependencyList) => {
  useEffect(apply, deps);
};

export const useTimeout = (run: () => void, delay: number | null) => {
  const fire = useEffectEvent(run);

  useEffect(() => {
    const timer = delay === null ? undefined : setTimeout(fire, delay);

    return () => {
      clearTimeout(timer);
    };
  }, [delay]);
};
