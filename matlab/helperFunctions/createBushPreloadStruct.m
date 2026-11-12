function [bushPreloadSigStruct,bushPreloadGradStruct]  = createBushPreloadStruct(bushStruct, steps, hpreload, signalType )
    % createBushStructPreloads
    % This function updates each element of the bushStruct array with preload
    % signal and preload gradient information.
    %
    % Inputs:
    %   bushStruct - An array of structures, each containing a 'preloads' field.
    %   steps - The number of steps for preload array creation.
    %   hpreload - A parameter used in the creation of preload arrays.
    %
    % Output:
    %   bushStruct - The updated array of structures with new 'preloadSig' and
    %                'preloadGrad' fields for each element.
    
    % Create empty bushPreloadStruct
    bushPreloadSigStruct = struct();
    bushPreloadGradStruct = struct();

    for i = 1:length(bushStruct)
        if bushStruct(i).active          
            [preloadSignal, preloadGrad] = createPreloadArrays(bushStruct(i).preloads, steps, hpreload, signalType);
            bushPreloadSigStruct(i).preloadCurrent = preloadSignal;
            bushPreloadGradStruct(i).preloadCurrent = preloadGrad;
        else
            bushPreloadSigStruct(i).preloads = [];
            bushPreloadGradStruct(i).preloads = [];
        end
    end
end
