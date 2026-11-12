function [preloadSignal, preloadGrad] = createPreloadArrays(preloadVec, steps, h, signalType)
    % createPreloadArrays generates two arrays based on preloadVec values and the specified signal type.
    %
    % Inputs:
    %   preloadVec - A vector of final preload values of size n.
    %   steps - The number of steps for the preload progression.
    %   h - A scalar value used for step calculation.
    %   signalType - A string specifying the type of signal ('linear' or 'sinusoidal').
    %
    % Outputs:
    %   preloadSignal - An array of size (n, steps) representing the signal progression.
    %   preloadGrad - An array of size (n, steps) representing the gradient.

    % Validate inputs
    if numel(preloadVec) ~= length(preloadVec)
        error('preloadVec must be a vector');
    end
    if ~isscalar(steps) || ~isscalar(h)
        error('Both steps and h must be scalar values');
    end
    if ~ischar(signalType) || ~ismember(signalType, {'linear', 'sinusoidal'})
        error('signalType must be either ''linear'' or ''sinusoidal''');
    end
    
    timeArray = (0:h:h*steps-h);

    % Generating stepArray based on signalType
    switch signalType
        case 'linear'
            stepArray = linspace(0, 1, steps);
        case 'sinusoidal'
            stepArray = sin(2 * pi * 1/30 * timeArray);
        otherwise
            error('Unsupported signal type');
    end

    % Generating preloadSignal
    preloadSignal = bsxfun(@times, preloadVec, stepArray);

    % Generating preloadGrad
    preloadGrad = gradient(preloadSignal, h);
end
