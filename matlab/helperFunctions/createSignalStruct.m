% createSignalStruct evaluates and differentiates signals from a drive structure
%
% Processes signals based on expressions in driveStructIn using specified time points,
% computes their gradients according to the chosen method, and organizes the output.
%
% Inputs:
%   driveStructIn - Structure containing signal expressions for each drive
%   time_array - Array of time points for signal evaluation and differentiation
%   switchGradMode - Determines gradient computation method: 'CentralDiff' or 'SplineDiff'
%
% Output:
%   signalStructOut - Structure array with fields 'name', 'signal_arr', and 'signal_grad'

function signalStructOut = createSignalStruct(driveStructIn, time_array, switchGradMode)
    % Initialize variables
    driveNames = fieldnames(driveStructIn);  % Names of drives
    numDrives = length(driveNames);  % Number of drives
    signal_length = length(time_array);  % Number of time points
    h = time_array(2) - time_array(1);  % Time step size for differentiation

    % Initialize output structure
    signalStructOut = repmat(struct('name', '', 'signal_array', [], 'signal_grad', []), numDrives, 1);

    % Create Time array for eval 
    TIME = time_array;
    time = time_array; 

    % Iterate through all drives
    for i = 1:numDrives
        driveName = driveNames{i};
        signals = driveStructIn.(driveName).signal;  % Extract signals for current drive
        signal_array = zeros(length(signals), signal_length);  % Initialize signal array for current drive

        if isnumeric(signals)
            signal_array(:,1:end) = repmat(signals, 1, signal_length);  % Assign numeric signals directly
        else
            % Evaluate each signal over the time array
            for j = 1:length(signals)
                if  ~strcmp(signals{j}, '~')  % Ignore placeholder signals
                    signal_array(j, :) = eval(signals{j});  % Evaluate signal
                end
            end
        end



        % Compute signal gradients based on specified mode
        switch switchGradMode
            case 'CentralDiff'
                signal_grad = gradient(signal_array, h);  % Central difference method

            case 'SplineDiff'
                pp = spline(time_array, signal_array);  % Cubic spline interpolation
                pp_der = fnder(pp);  % Derive spline
                signal_grad = fnval(pp_der, time_array);  % Evaluate derivative
        end

        % Store results in the output structure
        signalStructOut(i).name = driveName;
        signalStructOut(i).signal_array = signal_array;
        signalStructOut(i).signal_grad = signal_grad;
    end
end
